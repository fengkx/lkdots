mod cli;
mod config;
mod crypto;
mod operations;
mod output;
mod path_util;
mod symlink_util;

use age::secrecy::{ExposeSecret, SecretString};
use anyhow::{Context, Result, anyhow};
use config::ConfigFileStruct;
use indicatif::{ProgressBar, ProgressStyle};
use log::debug;
use operations::Op;
use path_util::{get_dir, pathbuf_to_str, relative_path};
use rayon::prelude::*;
use rpassword::prompt_password;
use std::{
    collections::HashMap,
    fs::{OpenOptions, read_to_string},
    io::{BufRead, ErrorKind, Seek, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
use walkdir::WalkDir;
use zeroize::Zeroize;

use crate::{
    config::Config,
    crypto::{decrypt_file, encrypt_file},
    operations::execute,
    output::{print_info, print_success},
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
            return Err(anyhow!(
                "Config file not found: {}\n\n\
                Hint: Use -c option to specify config file path\n\
                Default: lkdots.toml in current directory",
                cfg.config
            ));
        }
        return Err(anyhow!(err));
    }
    let config: Config = toml::from_str::<ConfigFileStruct>(&cfg_str?)?.into();

    // Validate configuration
    config
        .validate()
        .context("Configuration validation failed")?;

    let base_dir = get_dir(Path::new(&cfg.config))?;
    let entries = &config.entries;

    if cfg.is_encrypt_cmd() || cfg.is_decrypt_cmd() {
        let phrase = SecretString::from(prompt_password("Passphrase: ")?);
        if cfg.is_encrypt_cmd() {
            let mut again_phrase = prompt_password("Input passphrase again: ")?;
            if !constant_time_eq(phrase.expose_secret(), &again_phrase) {
                again_phrase.zeroize();
                return Err(anyhow!("Passphrase verification failed"));
            }
            again_phrase.zeroize();
        }

        // Phase 1: Collect files to process
        let files = collect_files_to_process(entries, cfg.is_encrypt_cmd())?;

        if files.is_empty() {
            print_info("No files to process.");
            return Ok(());
        }

        // Phase 2: Process files in parallel (with progress bar)
        let phrase_arc = Arc::new(phrase);
        let result = if cfg.is_encrypt_cmd() {
            encrypt_files_parallel(files, phrase_arc)
        } else {
            decrypt_files_parallel(files, phrase_arc)
        };

        return result;
    }

    let r = entries
        .par_iter()
        .filter(|e| e.match_platform())
        .map(|cfg| cfg.create_ops(base_dir));
    let opss = r
        .collect::<Result<Vec<Vec<Op>>>>()
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
        .truncate(false)
        .open(gitignore_path.as_ref())?;

    // Read existing content
    let reader = std::io::BufReader::new(&f);
    for line in reader.lines() {
        let line = line?;
        has_written.insert(line, true);
    }

    // Reposition to end of file for appending new content
    f.seek(std::io::SeekFrom::End(0))?;

    for e in cfg.entries.iter().filter(|&e| e.encrypt) {
        let relative = relative_path(shellexpand::tilde(e.from.as_ref()).as_ref(), dir)
            .context("Failed to calculate relative path for gitignore entry")?;
        let p = relative.to_string_lossy();
        let patterns = vec![format!("{}/*", p), format!("!{}/*.enc", p)];
        for s in patterns {
            if !has_written.contains_key(&s) {
                if simulate {
                    println!("{}", s);
                } else {
                    writeln!(f, "{}", s).context("Fail to write gitignore")?;
                }
            }
        }
    }

    Ok(())
}

/// Constant-time string comparison to prevent timing attacks
/// Even if passwords don't match, the full comparison operation is performed
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }

    // Use byte-level constant-time comparison
    a.bytes()
        .zip(b.bytes())
        .map(|(x, y)| x ^ y)
        .fold(0u8, |acc, diff| acc | diff)
        == 0
}

/// Collect list of files that need to be encrypted or decrypted
fn collect_files_to_process(
    entries: &[crate::config::Entry],
    is_encrypt: bool,
) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for entry in entries.iter().filter(|e| e.encrypt) {
        let expanded_from = shellexpand::tilde(entry.from.as_ref());
        let walker = WalkDir::new(expanded_from.as_ref())
            .follow_links(false)
            .into_iter();

        for entry_result in walker.filter_entry(|e| !e.path_is_symlink()) {
            let entry = entry_result?;
            if entry.metadata()?.is_file() {
                let path = entry.path();
                let path_str = path.to_string_lossy();

                if is_encrypt {
                    // Encryption: skip already encrypted files
                    if !path_str.ends_with(".enc") {
                        files.push(path.to_path_buf());
                    }
                } else {
                    // Decryption: only process .enc files
                    if path_str.ends_with(".enc") {
                        files.push(path.to_path_buf());
                    }
                }
            }
        }
    }

    Ok(files)
}

/// Encrypt files in parallel with progress bar
fn encrypt_files_parallel(files: Vec<PathBuf>, passphrase: Arc<SecretString>) -> Result<()> {
    let total = files.len();
    if total == 0 {
        return Ok(());
    }

    // Create progress bar
    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} files ({percent}%) {msg}")
            .unwrap()
            .progress_chars("#>-"),
    );

    // Process files in parallel (ProgressBar is thread-safe and can be cloned directly)
    files.par_iter().try_for_each(|file_path| -> Result<()> {
        let file_str = file_path.to_string_lossy();
        let pb = pb.clone();

        // Update progress bar message
        let display_name = if file_str.len() > 50 {
            format!("...{}", &file_str[file_str.len() - 47..])
        } else {
            file_str.to_string()
        };
        pb.set_message(display_name);

        // Execute encryption
        encrypt_file(&file_str, &passphrase)?;
        pb.inc(1);
        Ok(())
    })?;

    pb.finish_with_message("Encryption completed");
    print_success(&format!("Successfully encrypted {} file(s)", total));

    Ok(())
}

/// Decrypt files in parallel with progress bar
fn decrypt_files_parallel(files: Vec<PathBuf>, passphrase: Arc<SecretString>) -> Result<()> {
    let total = files.len();
    if total == 0 {
        return Ok(());
    }

    // Create progress bar
    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} files ({percent}%) {msg}")
            .unwrap()
            .progress_chars("#>-"),
    );

    // Process files in parallel (ProgressBar is thread-safe and can be cloned directly)
    files.par_iter().try_for_each(|file_path| -> Result<()> {
        let file_str = file_path.to_string_lossy();
        let pb = pb.clone();

        // Update progress bar message
        let display_name = if file_str.len() > 50 {
            format!("...{}", &file_str[file_str.len() - 47..])
        } else {
            file_str.to_string()
        };
        pb.set_message(display_name);

        // Execute decryption
        decrypt_file(&file_str, &passphrase)?;
        pb.inc(1);
        Ok(())
    })?;

    pb.finish_with_message("Decryption completed");
    print_success(&format!("Successfully decrypted {} file(s)", total));

    Ok(())
}
