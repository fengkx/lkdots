mod cli;
mod config;
mod operations;
mod path_util;
mod symlink_util;

use anyhow::{Context, Result};
use config::{ConfigEntry, ConfigStruct};
use operations::Op;
use path_util::get_dir;
use rayon::prelude::*;
use std::{
    ffi::OsString,
    fs::{read_dir, read_link, read_to_string},
    path::Path,
    process,
};

use crate::path_util::relative_path;

#[macro_use]
extern crate lazy_static;

fn main() -> Result<()> {
    let cfg = cli::config()?;
    let cfg_str = read_to_string(&cfg.config)?;
    let config = toml::from_str::<ConfigStruct>(&cfg_str).map(|cfg| cfg.entries)?;
    let base_dir = get_dir(Path::new(&cfg.config))?;

    let r = config
        .par_iter()
        .map(|cfg: &ConfigEntry| -> Result<Vec<Op>> {
            let from_osstr: OsString = if cfg.from.starts_with("/") || cfg.from.starts_with("~") {
                cfg.from.clone().into()
            } else {
                base_dir.join(&cfg.from).into_os_string()
            };
            let from = from_osstr.to_str().unwrap();
            let from = shellexpand::tilde(from);
            let to = shellexpand::tilde(&cfg.to);
            // println!("from: {}, to: {}", from, to);
            let mut result = Vec::<Op>::new();
            link_file_or_dir(from.as_ref(), to.as_ref(), &mut result)?;
            // println!("{:?} {:?}", res, cfg);
            Ok(result)
        });
    let ops = r.collect::<Result<Vec<Vec<Op>>>>().unwrap();
    println!("{:?}", ops);
    Ok(())
}

fn link_file_or_dir(from: &str, to: &str, result: &mut Vec<Op>) -> Result<()> {
    let metadata = Path::new(to).symlink_metadata();
    if metadata.is_ok() && !metadata.as_ref().unwrap().is_dir() {
        // file existed
        let metadata = metadata.unwrap();
        if metadata.is_symlink() {
            let sym_target = std::fs::canonicalize(to)?;
            let sym_target = sym_target.to_str().context("Fail to get str path")?;
            let abs_from = std::fs::canonicalize(from)?;
            let abs_from = abs_from.to_str().context("Fail to get str path")?;
            if sym_target != abs_from {
                println!("{}\n{}", sym_target, sym_target);
                result.push(Op::Conflict(to.to_string()));
            } else {
                result.push(Op::Existed(to.to_string()));
            }
        } else {
            result.push(Op::Conflict(to.to_string()));
        }
    } else {
        let from_path = Path::new(from);
        if from_path.symlink_metadata()?.is_dir() {
            link_dir(from, to, result)?;
        } else {
            link_file(from, to, result)?;
        };
    }
    Ok(())
}

fn link_file(from: &str, to: &str, res: &mut Vec<Op>) -> Result<()> {
    let to_dir = Path::new(to)
        .parent()
        .context("Not parent dir")?
        .to_str()
        .context("Fail to get str path")?;
    let relative = relative_path(from, to_dir)?;

    res.push(Op::Symlink(
        relative.to_string_lossy().to_string(),
        to.to_owned(),
    ));
    Ok(())
}

fn link_dir(from: &str, to: &str, result: &mut Vec<Op>) -> Result<()> {
    let relative = relative_path(from, to)?;

    let to_path = Path::new(to);
    if !to_path.exists() {
        // create_dir_all(to_path.parent().unwrap_or(Path::new("/")))?;
        result.push(Op::Mkdirp(
            to_path
                .parent()
                .unwrap_or(Path::new("/"))
                .to_str()
                .unwrap()
                .into(),
        ));
        result.push(Op::Symlink(
            relative.to_str().context("Fail to get str path")?.into(),
            to.into(),
        ));
    } else {
        // directory existed, link files in directory
        for f in read_dir(from)?.into_iter() {
            let f = f?;
            let from_path = f.path().to_path_buf();
            let from_str = from_path.to_str().context("Fail to get str path")?;

            let fname = f.file_name();
            let fname = fname.to_str().context("Fail to get str path")?;

            let to_path = Path::new(to).join(fname);

            let to_str = to_path.to_str().context("Fail to get str path")?;

            // println!("{:?} {:?}", from_path, to_str);
            link_file_or_dir(from_str, to_str, result)?;
        }
    }
    Ok(())
}
