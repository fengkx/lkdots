use std::{env::current_dir, io::Result};
use structopt::StructOpt;

lazy_static! {
    static ref LKDOTS_DEFAULT_CONFIG_PATH: String = current_dir()
        .map(|p| { p.join("lkdots.toml") })
        .map(|p| { p.to_str().unwrap().clone().to_owned() })
        .expect("Fail to found current dir");
}

#[derive(PartialEq, StructOpt, Debug)]
/// Cli
pub struct Cli {
    /// path to config file
    #[structopt(short = "c", default_value = &LKDOTS_DEFAULT_CONFIG_PATH)]
    pub config: String,

    /// simulate fs operations, do not actually make any filesystem changes
    #[structopt(long = "simulate")]
    pub simulate: bool,
}

pub fn config() -> Result<Cli> {
    let args = Cli::from_args();
    Ok(args)
}

#[test]
fn test_config_init() {
    println!("{:?}", config().unwrap())
}
