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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crypto() {
        let passphrase = "abc";
        let p = "./tests/test-data/private.key";
        let original = std::fs::read_to_string(p).unwrap();
        let encrypted_path = format!("{}.enc", p);
        encrypt_file(p, passphrase).unwrap();
        decrypt_file(&encrypted_path, passphrase).unwrap();
        let encrypted_str = std::fs::read_to_string(encrypted_path).unwrap_or("".to_string());
        let decrypted_str = std::fs::read_to_string(p).unwrap();
        assert_eq!(original, decrypted_str);
        assert_ne!(original, encrypted_str)
    }
}
