mod cli;
mod config;
mod symlink;

use config::{ConfigEntry, ConfigStruct};
use std::{error::Error, vec};
#[macro_use]
extern crate lazy_static;

fn main() -> Result<(), Box<dyn Error>> {
    let entries = vec![ConfigEntry::new("a", "b"), ConfigEntry::new("c", "d")];
    let cfg = ConfigStruct { entries };
    let toml = toml::to_string(&cfg).unwrap();
    println!("{}", toml);
    let cfg = cli::config()?;
    println!("{:?}", cfg);
    Ok(())
}
