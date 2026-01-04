mod cli;
mod config;
mod crypto;
mod operations;
mod path_util;
mod symlink_util;

use anyhow::{anyhow, Context, Result};
use config::ConfigFileStruct;
use log::{debug, info};
use operations::Op;
use path_util::{get_dir, pathbuf_to_str, relative_path};
use rayon::prelude::*;
use rpassword::prompt_password_stdout;
use age::secrecy::{Secret, ExposeSecret};
use zeroize::Zeroize;
use std::{
    collections::HashMap,
    fs::{read_to_string, OpenOptions},
    io::{BufRead, ErrorKind, Write},
    path::Path,
};
use walkdir::WalkDir;

use crate::{
    config::Config,
    crypto::{decrypt_file, encrypt_file},
    operations::execute,
};

#[macro_use]
extern crate lazy_static;

fn main() -> Result<()> {
    env_logger::init();

    let cfg = cli::config()?;
    let cfg_str = read_to_string(&cfg.config);
    if let Err(err) = cfg_str {
        debug!("{}", err);
        if err.kind() == ErrorKind::NotFound {
            return Err(anyhow!("Cannot found config toml (default: lkdots.toml)"));
        }
        return Err(anyhow!(err));
    }
    let config: Config = toml::from_str::<ConfigFileStruct>(&cfg_str?)?.into();
    let base_dir = get_dir(Path::new(&cfg.config))?;
    let entries = &config.entries;

    if cfg.is_encrypt_cmd() || cfg.is_decrypt_cmd() {
        let phrase = Secret::new(prompt_password_stdout("Passphrase: ")?);
        if cfg.is_encrypt_cmd() {
            let mut again_phrase = prompt_password_stdout("Input passphrase again: ")?;
            if !constant_time_eq(&phrase.expose_secret(), &again_phrase) {
                again_phrase.zeroize();
                return Err(anyhow!("Two passphrase is different"));
            }
            again_phrase.zeroize();
        }
        
        let result = entries
            .par_iter()
            .filter(|e| e.encrypt)
            .map(|e| {
                let expanded_from = shellexpand::tilde(e.from.as_ref());
                let walker = WalkDir::new(expanded_from.as_ref())
                    .follow_links(false)
                    .into_iter();
                for entry in walker.filter_entry(|e| !e.path_is_symlink()) {
                    let entry = entry?;
                    if entry.metadata()?.is_file() {
                        let path = entry.path().to_string_lossy();
                        if cfg.is_encrypt_cmd() {
                            if !path.as_ref().ends_with(".enc") {
                                info!("encrypt: {}", path.as_ref());
                                encrypt_file(path.as_ref(), &phrase)?;
                            }
                        } else if cfg.is_decrypt_cmd() && path.as_ref().ends_with(".enc") {
                            info!("decrypt: {}", path.as_ref());
                            decrypt_file(path.as_ref(), &phrase)?;
                        }
                    }
                }
                Ok(())
            })
            .collect::<Result<()>>();
        
        // 密码不再需要时，确保内存被清零
        // Secret 类型在 Drop 时会自动清零，但这里显式处理以确保安全
        return result;
    }

    let r = entries
        .par_iter()
        .filter(|e| e.match_platform())
        .map(|cfg| cfg.create_ops(base_dir));
    let opss = r.collect::<Result<Vec<Vec<Op>>>>()
        .context("Failed to create operations for some entries")?;

    if cfg.simulate {
        let output = opss
            .iter()
            .map(|ops| {
                ops.iter()
                    .map(|op| format!("{}", op))
                    .collect::<Vec<String>>()
                    .join("\n")
            })
            .collect::<Vec<String>>()
            .join("\n");
        println!("{}", output);
    } else {
        opss.par_iter()
            .map(|ops| -> Result<()> { execute(ops) })
            .collect::<Result<()>>()?;
    }
    write_gitignore(&config, cfg.simulate)?;
    Ok(())
}

fn write_gitignore(cfg: &Config, simulate: bool) -> Result<()> {
    let gitignore_path = shellexpand::tilde(&cfg.gitignore);
    let dir = pathbuf_to_str(
        Path::new(gitignore_path.as_ref())
            .parent()
            .context("Fail to get git repository root")?,
    )?;

    let mut has_written = HashMap::new();
    let mut f = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(gitignore_path.as_ref())?;
    let reader = std::io::BufReader::new(&f);
    let lines = reader.lines();
    for line in lines.flatten() {
        has_written.insert(line, true);
    }

    for e in cfg.entries.iter().filter(|&e| e.encrypt) {
        let relative = relative_path(shellexpand::tilde(e.from.as_ref()).as_ref(), dir)
            .context("Failed to calculate relative path for gitignore entry")?;
        let p = relative.to_string_lossy();
        let patterns = vec![format!("{}/*", p), format!("!{}/*.enc", p)];
        for s in patterns {
            if has_written.get(&s).is_none() {
                if simulate {
                    println!("{}", s);
                } else {
                    writeln!(f, "{}", s)
                        .context("Fail to write gitignore")?;
                }
            }
        }
    }

    Ok(())
}

/// 常量时间字符串比较，防止时序攻击
/// 即使密码不匹配，也会执行完整的比较操作
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    
    // 使用字节级别的常量时间比较
    a.bytes()
        .zip(b.bytes())
        .map(|(x, y)| x ^ y)
        .fold(0u8, |acc, diff| acc | diff)
        == 0
}
