mod cli;
mod config;
mod path_util;
mod symlink_util;

use anyhow::Result;
use config::{ConfigEntry, ConfigStruct};
use path_util::get_dir;
use pathdiff::diff_paths;
use rayon::prelude::*;
use std::path::Path;
use std::{env, error::Error, fs::read_to_string, process};
use symlink_util::create_symlink;

#[macro_use]
extern crate lazy_static;

fn main() -> Result<()> {
    let cfg = cli::config()?;
    let cfg_str = read_to_string(&cfg.config)?;
    let config = toml::from_str::<ConfigStruct>(&cfg_str).map(|cfg| cfg.entries);
    if let Err(err) = config {
        eprintln!("{}", err);
        process::exit(1);
    }
    let config = config.unwrap();
    let base_dir = get_dir(Path::new(&cfg.config))?;

    let r = config.par_iter().map(|cfg: &ConfigEntry| -> Result<()> {
        let from_osstr = base_dir.join(&cfg.from).into_os_string();
        let from = from_osstr.to_str().unwrap();
        let from = shellexpand::tilde(from);
        let to = shellexpand::tilde(&cfg.to);

        Ok(())
    });
    let _r = r.collect::<Result<()>>();
    Ok(())
}

fn link_file_or_dir(from: &str, to: &str) -> Result<()> {
    // for entry in walkdir::WalkDir::new(from).follow_links(false).into_iter() {
    //     println!("path: {:?}", entry?.path());
    // }
    Ok(())
}

fn link_file(from: &str, to: &str) -> Result<()> {
    let relative = diff_paths(from, to).expect(&format!(
        "Fail to find relative path from {} to {}",
        from, to
    ));
    println!("{} -> {}, {:?}", from, to, relative);
    create_symlink((relative).to_string_lossy().as_ref(), &to)?;
    Ok(())
}
fn link_dir(from: &str, to: &str) -> Result<()> {
    unimplemented!()
}
