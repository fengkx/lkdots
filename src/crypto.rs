use age::cli_common::file_io::{OutputFormat, OutputWriter};
use age::secrecy::SecretString;
use anyhow::{Result, anyhow};
use log::debug;
use std::fs::OpenOptions;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

pub fn encrypt_file(src: &str, passphrase: &SecretString) -> Result<()> {
    debug!("encrypting file: {}", src);
    let mut reader = OpenOptions::new().read(true).open(src)?;
    let encryptor = age::Encryptor::with_user_passphrase(passphrase.clone());
    let writer = OutputWriter::new(
        Some(format!("{}.enc", src)),
        true, // allow_overwrite
        OutputFormat::Text,
        0o644,
        false, // input_is_tty
    )?;
    let mut writer = encryptor.wrap_output(writer)?;

    io::copy(&mut reader, &mut writer)?;
    writer.finish()?;

    Ok(())
}

pub fn decrypt_file(src: &str, passphrase: &SecretString) -> Result<()> {
    use std::path::Path;

    let path = Path::new(src);
    let strip_fname = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("Invalid encrypted file name: {}", src))?;
    let strip_fname = path
        .parent()
        .map(|p| p.join(strip_fname))
        .unwrap_or_else(|| Path::new(strip_fname).to_path_buf());
    let strip_fname = strip_fname
        .to_str()
        .ok_or_else(|| anyhow!("Invalid path encoding"))?;

    let encrypted_file = OpenOptions::new().create(false).read(true).open(src)?;
    let decryptor = age::Decryptor::new(encrypted_file)?;

    let mut decrypted = {
        let mut op = OpenOptions::new();

        op.create(true).write(true);

        if cfg!(unix) {
            op.mode(0o600);
        }
        op.open(strip_fname)?
    };

    let identity = age::scrypt::Identity::new(passphrase.clone());
    let mut reader = decryptor.decrypt(std::iter::once(&identity as &dyn age::Identity))?;
    io::copy(&mut reader, &mut decrypted)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crypto() {
        let passphrase = SecretString::new("abc".to_string().into_boxed_str());
        let p = "./tests/test-data/private.key";
        let original = std::fs::read_to_string(p).unwrap();
        let encrypted_path = format!("{}.enc", p);
        encrypt_file(p, &passphrase).unwrap();
        decrypt_file(&encrypted_path, &passphrase).unwrap();
        let encrypted_str =
            std::fs::read_to_string(encrypted_path).unwrap_or_else(|_| "".to_string());
        let decrypted_str = std::fs::read_to_string(p).unwrap();
        assert_eq!(original, decrypted_str);
        assert_ne!(original, encrypted_str)
    }
}
