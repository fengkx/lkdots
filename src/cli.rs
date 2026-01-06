use clap::{Parser, Subcommand};
use log::debug;
use std::{env::current_dir, io::Result, sync::LazyLock};

static LKDOTS_DEFAULT_CONFIG_PATH: LazyLock<String> = LazyLock::new(|| {
    current_dir()
        .map(|p| p.join("lkdots.toml"))
        .and_then(|p| {
            p.to_str().map(|s| s.to_owned()).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Current directory path contains invalid UTF-8",
                )
            })
        })
        .expect("Fail to found current dir")
});

#[derive(PartialEq, Parser, Debug)]
#[command(
    version,
    about = "A cli tool to create symbol link of dotfiles with encryption and more"
)]
pub struct Cli {
    /// path to config file
    #[arg(short = 'c', default_value = LKDOTS_DEFAULT_CONFIG_PATH.as_str())]
    pub config: String,

    /// simulate fs operations, do not actually make any filesystem changes
    #[arg(long = "simulate")]
    pub simulate: bool,

    #[command(subcommand)]
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

#[derive(Subcommand, PartialEq, Debug)]
pub enum SubCommand {
    /// encrypt files to *.enc file
    Encrypt,
    /// decrypt files to original position
    Decrypt,
}

pub fn config() -> Result<Cli> {
    let args = Cli::parse();
    debug!("{:?}", args);
    Ok(args)
}

#[test]
fn test_config_init() {
    println!("{:?}", config().unwrap())
}

#[test]
fn test_is_encrypt_cmd() {
    use clap::Parser;
    let cli = Cli::parse_from(&["lkdots", "encrypt"]);
    assert!(cli.is_encrypt_cmd());
    assert!(!cli.is_decrypt_cmd());

    let cli = Cli::parse_from(&["lkdots", "decrypt"]);
    assert!(!cli.is_encrypt_cmd());
    assert!(cli.is_decrypt_cmd());

    let cli = Cli::parse_from(&["lkdots"]);
    assert!(!cli.is_encrypt_cmd());
    assert!(!cli.is_decrypt_cmd());
}

#[test]
fn test_is_decrypt_cmd() {
    use clap::Parser;
    let cli = Cli::parse_from(&["lkdots", "decrypt"]);
    assert!(cli.is_decrypt_cmd());
    assert!(!cli.is_encrypt_cmd());
}
