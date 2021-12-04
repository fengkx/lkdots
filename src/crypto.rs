use age::cli_common::file_io::{OutputFormat, OutputWriter};
use age::secrecy::Secret;
use anyhow::Result;
use log::debug;
use std::fs::OpenOptions;
use std::io;

pub fn encrypt_file(src: &str, passphrase: &str) -> Result<()> {
    debug!("passphrase length: {}", passphrase.len());
    let mut reader = OpenOptions::new().read(true).open(src)?;
    let encryptor = age::Encryptor::with_user_passphrase(Secret::new(passphrase.to_owned()));
    let writer = OutputWriter::new(Some(format!("{}.enc", src)), OutputFormat::Text, 0o644)?;
    let mut writer = encryptor.wrap_output(writer)?;

    io::copy(&mut reader, &mut writer)?;
    writer.finish()?;

    Ok(())
}

pub fn decrypt_file(src: &str, passphrase: &str) -> Result<()> {
    let strip_fname = &src[0..src.len() - 4];
    let encrypted_file = OpenOptions::new().create(false).read(true).open(src)?;
    let decryptor = match age::Decryptor::new(encrypted_file)? {
        age::Decryptor::Passphrase(d) => d,
        _ => unreachable!(),
    };

    let mut decrypted = OpenOptions::new()
        .create(true)
        .write(true)
        .open(strip_fname)?;
    let mut reader = decryptor.decrypt(&Secret::new(passphrase.to_owned()), None)?;
    io::copy(&mut reader, &mut decrypted)?;
    Ok(())
}
