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

    #[structopt(subcommand)]
    pub cmd: Option<SubCommand>,
}

impl Cli {
    pub fn is_encrypt_cmd(&self) -> bool {
        match self.cmd.as_ref() {
            Some(SubCommand::Encrypt) => true,
            Some(SubCommand::Decrypt) => false,
            None => false,
        }
    }
    pub fn is_decrypt_cmd(&self) -> bool {
        match self.cmd.as_ref() {
            Some(SubCommand::Encrypt) => false,
            Some(SubCommand::Decrypt) => true,
            None => false,
        }
    }
}

#[derive(StructOpt, PartialEq, Debug)]
pub enum SubCommand {
    /// encrypt files to *.enc
    Encrypt,
    /// decrypt files original position
    Decrypt,
}

pub fn config() -> Result<Cli> {
    let args = Cli::from_args();
    println!("{:?}", args);
    Ok(args)
}

#[test]
fn test_config_init() {
    println!("{:?}", config().unwrap())
}
