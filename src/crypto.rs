use age::armor::{ArmoredReader, ArmoredWriter, Format};
use age::cli_common::file_io::{OutputFormat, OutputWriter};
use age::secrecy::SecretString;
use anyhow::{Context, Result, anyhow};
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
        0o600,
        false, // input_is_tty
    )?;
    let armored = ArmoredWriter::wrap_output(writer, Format::AsciiArmor)?;
    let mut writer = encryptor.wrap_output(armored)?;

    io::copy(&mut reader, &mut writer)?;
    writer.finish()?.finish()?;

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

    let encrypted_file = OpenOptions::new()
        .create(false)
        .read(true)
        .open(src)
        .with_context(|| format!("Failed to open encrypted file for reading: {}", src))?;
    // ArmoredReader auto-detects whether the input is ASCII-armored or binary age format.
    let decryptor = age::Decryptor::new(ArmoredReader::new(encrypted_file))?;

    debug!("decrypting file: {} to {}", src, strip_fname);

    let mut decrypted = {
        let mut op = OpenOptions::new();

        // Overwrite the target file contents (if it exists).
        op.create(true).write(true).truncate(true);

        if cfg!(unix) {
            op.mode(0o600);
        }
        op.open(strip_fname).with_context(|| {
            format!(
                "Failed to open decrypted output file for writing (permission denied?): {}",
                strip_fname
            )
        })?
    };

    let identity = age::scrypt::Identity::new(passphrase.clone());
    let mut reader = decryptor.decrypt(std::iter::once(&identity as &dyn age::Identity))?;
    io::copy(&mut reader, &mut decrypted)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_crypto() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test_file.txt");
        let original = "test content for encryption";
        fs::write(&test_file, original).unwrap();

        let passphrase = SecretString::new("abc".to_string().into_boxed_str());
        let p = test_file.to_str().unwrap();
        let encrypted_path = format!("{}.enc", p);
        encrypt_file(p, &passphrase).unwrap();
        decrypt_file(&encrypted_path, &passphrase).unwrap();
        let encrypted_str = fs::read_to_string(&encrypted_path).unwrap();
        assert!(
            encrypted_str.starts_with("-----BEGIN AGE ENCRYPTED FILE-----"),
            "encrypted output should be ASCII-armored"
        );
        assert!(
            encrypted_str.contains("-----END AGE ENCRYPTED FILE-----"),
            "encrypted output should contain END marker"
        );
        let decrypted_str = fs::read_to_string(p).unwrap();
        assert_eq!(original, decrypted_str);
        assert_ne!(original, encrypted_str)
    }

    #[test]
    fn test_encrypt_file_nonexistent() {
        let passphrase = SecretString::new("test".to_string().into_boxed_str());
        let result = encrypt_file("/nonexistent/file.txt", &passphrase);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_file_invalid_filename() {
        let temp_dir = TempDir::new().unwrap();
        let invalid_file = temp_dir.path().join("invalid");
        fs::write(&invalid_file, "not a valid age encrypted file").unwrap();

        let passphrase = SecretString::new("test".to_string().into_boxed_str());
        let result = decrypt_file(invalid_file.to_str().unwrap(), &passphrase);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_file_nonexistent() {
        let passphrase = SecretString::new("test".to_string().into_boxed_str());
        let result = decrypt_file("/nonexistent/file.enc", &passphrase);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_file_corrupted() {
        let temp_dir = TempDir::new().unwrap();
        let corrupt_file = temp_dir.path().join("corrupt.enc");
        fs::write(&corrupt_file, "this is not a valid age file").unwrap();

        let passphrase = SecretString::new("test".to_string().into_boxed_str());
        let result = decrypt_file(corrupt_file.to_str().unwrap(), &passphrase);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_file_wrong_passphrase() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test_file.txt");
        fs::write(&test_file, "test content").unwrap();

        let passphrase = SecretString::new("correct".to_string().into_boxed_str());
        let encrypted_path = format!("{}.enc", test_file.to_str().unwrap());
        encrypt_file(test_file.to_str().unwrap(), &passphrase).unwrap();

        let wrong_passphrase = SecretString::new("wrong".to_string().into_boxed_str());
        let result = decrypt_file(&encrypted_path, &wrong_passphrase);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_file_empty_passphrase() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test_file.txt");
        fs::write(&test_file, "test content").unwrap();

        let passphrase = SecretString::new("test".to_string().into_boxed_str());
        let encrypted_path = format!("{}.enc", test_file.to_str().unwrap());
        encrypt_file(test_file.to_str().unwrap(), &passphrase).unwrap();

        let empty_passphrase = SecretString::new("".to_string().into_boxed_str());
        let result = decrypt_file(&encrypted_path, &empty_passphrase);
        assert!(result.is_err());
    }

    #[test]
    fn test_encrypt_file_with_special_characters() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("tëst_fîlé.txt");
        let content = "测试内容 with émojis 🎉 and spécial chars";
        fs::write(&test_file, content).unwrap();

        let passphrase = SecretString::new("test".to_string().into_boxed_str());
        encrypt_file(test_file.to_str().unwrap(), &passphrase).unwrap();

        let encrypted_path_str = format!("{}.enc", test_file.to_str().unwrap());
        let encrypted_path = std::path::PathBuf::from(&encrypted_path_str);
        assert!(encrypted_path.exists());

        decrypt_file(&encrypted_path_str, &passphrase).unwrap();
        let decrypted = fs::read_to_string(&test_file).unwrap();
        assert_eq!(content, decrypted);
    }

    #[test]
    fn test_decrypt_file_no_extension() {
        let temp_dir = TempDir::new().unwrap();
        let file_no_ext = temp_dir.path().join("file_no_extension");
        fs::write(&file_no_ext, "some content").unwrap();

        let passphrase = SecretString::new("test".to_string().into_boxed_str());
        let result = decrypt_file(file_no_ext.to_str().unwrap(), &passphrase);
        assert!(result.is_err());
        // Just check that it returns an error for invalid file name
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_file_utf8_path() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("测试文件.txt");
        fs::write(&test_file, "utf8 content").unwrap();

        let passphrase = SecretString::new("test".to_string().into_boxed_str());
        let encrypted_path = format!("{}.enc", test_file.to_str().unwrap());
        encrypt_file(test_file.to_str().unwrap(), &passphrase).unwrap();

        decrypt_file(&encrypted_path, &passphrase).unwrap();
        let decrypted = fs::read_to_string(&test_file).unwrap();
        assert_eq!("utf8 content", decrypted);
    }

    #[test]
    fn test_encrypt_empty_file() {
        let temp_dir = TempDir::new().unwrap();
        let empty_file = temp_dir.path().join("empty.txt");
        fs::write(&empty_file, "").unwrap();

        let passphrase = SecretString::new("test".to_string().into_boxed_str());
        encrypt_file(empty_file.to_str().unwrap(), &passphrase).unwrap();

        let encrypted_path_str = format!("{}.enc", empty_file.to_str().unwrap());
        let encrypted_path = std::path::PathBuf::from(&encrypted_path_str);
        assert!(encrypted_path.exists());

        decrypt_file(&encrypted_path_str, &passphrase).unwrap();
        let decrypted = fs::read_to_string(&empty_file).unwrap();
        assert_eq!("", decrypted);
    }
}
