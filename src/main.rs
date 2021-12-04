mod cli;
mod config;
mod crypto;
mod operations;
mod path_util;
mod symlink_util;

use anyhow::{anyhow, Result};
use config::ConfigFileStruct;
use log::{debug, info};
use operations::Op;
use path_util::get_dir;
use rayon::prelude::*;
use rpassword::prompt_password_stdout;
use std::{fs::read_to_string, path::Path};
use walkdir::WalkDir;

use crate::{
    config::Config,
    crypto::{decrypt_file, encrypt_file},
    operations::excute,
};

#[macro_use]
extern crate lazy_static;

fn main() -> Result<()> {
    env_logger::init();
    let cfg = cli::config()?;
    let cfg_str = read_to_string(&cfg.config)?;
    let config: Config = toml::from_str::<ConfigFileStruct>(&cfg_str)?.into();
    debug!("{:?}", config);
    let base_dir = get_dir(Path::new(&cfg.config))?;
    let entries = config.entries;

    if cfg.is_encrypt_cmd() || cfg.is_decrypt_cmd() {
        let phrase = prompt_password_stdout("Passphrase: ")?;
        if cfg.is_encrypt_cmd() {
            let again_phrase = prompt_password_stdout("Input passphrase again: ")?;
            if again_phrase != phrase {
                return Err(anyhow!("Two passphrase is different"));
            }
        }
        return entries
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
                        } else if cfg.is_decrypt_cmd() {
                            if path.as_ref().ends_with(".enc") {
                                info!("decrypt: {}", path.as_ref());
                                decrypt_file(path.as_ref(), &phrase)?;
                            }
                        }
                    }
                }
                Ok(())
            })
            .collect::<Result<()>>();
    }

    let r = entries
        .par_iter()
        .filter(|e| e.match_platform())
        .map(|cfg| cfg.create_ops(base_dir));
    let opss = r.collect::<Result<Vec<Vec<Op>>>>().unwrap();
    debug!("{:?}", opss);
    if !cfg.simulate {
        opss.par_iter()
            .map(|ops| -> Result<()> { excute(ops) })
            .collect::<Result<()>>()?;
    }
    Ok(())
}
