mod cli;
mod config;
mod operations;
mod path_util;
mod symlink_util;

use anyhow::Result;
use config::ConfigFileStruct;
use log::debug;
use operations::Op;
use path_util::get_dir;
use rayon::prelude::*;
use std::{fs::read_to_string, path::Path};

use crate::{config::Config, operations::excute};

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

    if cfg.is_encrypt_cmd() {
        return entries.par_iter().filter(|e| e.encrypt);
        .map(|e| {
            
        })
        .collect();
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
