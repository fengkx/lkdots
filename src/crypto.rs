use crate::path_util::{FileModePolicy, atomic_write};
use age::armor::{ArmoredReader, ArmoredWriter, Format};
use age::secrecy::SecretString;
use anyhow::{Context, Result, anyhow, ensure};
use log::debug;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

const ENCRYPTED_SUFFIX: &str = ".enc";

fn encrypted_path_for(src: &Path) -> Result<PathBuf> {
    ensure!(
        !src.as_os_str().is_empty(),
        "Source file path cannot be empty"
    );
    let mut path = src.as_os_str().to_os_string();
    path.push(ENCRYPTED_SUFFIX);
    Ok(PathBuf::from(path))
}

fn decrypted_path_for(src: &Path) -> Result<PathBuf> {
    let file_name = src
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("Invalid encrypted file name: {}", src.display()))?;

    let plain_name = file_name
        .strip_suffix(ENCRYPTED_SUFFIX)
        .filter(|name| !name.is_empty() && *name != "." && *name != "..")
        .ok_or_else(|| {
            anyhow!(
                "Invalid encrypted file name (expected <filename>{ENCRYPTED_SUFFIX}): {}",
                src.display()
            )
        })?;

    Ok(src.with_file_name(plain_name))
}

pub fn encrypt_file(src: &str, passphrase: &SecretString) -> Result<()> {
    debug!("encrypting file: {src}");
    let src_path = Path::new(src);
    let enc_path = encrypted_path_for(src_path)?;

    let mut reader = File::open(src_path)
        .with_context(|| format!("Failed to open source file for encryption: {src}"))?;

    atomic_write(&enc_path, FileModePolicy::Restricted0600, |out_file| {
        let encryptor = age::Encryptor::with_user_passphrase(passphrase.clone());
        let armored = ArmoredWriter::wrap_output(out_file, Format::AsciiArmor)?;
        let mut writer = encryptor.wrap_output(armored).map_err(io::Error::other)?;
        io::copy(&mut reader, &mut writer)?;
        writer.finish()?.finish()?;
        Ok(())
    })
    .with_context(|| {
        format!(
            "Failed to atomically write encrypted file: {}",
            enc_path.display()
        )
    })
}

pub fn decrypt_file(src: &str, passphrase: &SecretString) -> Result<()> {
    let src_path = Path::new(src);
    let target_path = decrypted_path_for(src_path)?;

    let encrypted_file = File::open(src_path)
        .with_context(|| format!("Failed to open encrypted file for reading: {src}"))?;
    // ArmoredReader auto-detects whether the input is ASCII-armored or binary age format.
    let decryptor = age::Decryptor::new(ArmoredReader::new(encrypted_file))
        .with_context(|| format!("Failed to read encrypted file header: {src}"))?;

    debug!("decrypting file: {src} to {}", target_path.display());

    let identity = age::scrypt::Identity::new(passphrase.clone());
    let mut reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .with_context(|| format!("Failed to decrypt file (wrong passphrase?): {src}"))?;

    atomic_write(&target_path, FileModePolicy::Restricted0600, |decrypted| {
        io::copy(&mut reader, decrypted).map(|_| ())
    })
    .with_context(|| {
        format!(
            "Failed to atomically write decrypted output file: {}",
            target_path.display()
        )
    })
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

    fn secret(s: &str) -> SecretString {
        SecretString::from(s.to_owned())
    }

    fn corrupt_armored_tail(armored: &str) -> String {
        let mut lines: Vec<String> = armored.lines().map(ToOwned::to_owned).collect();
        let payload_idx = lines
            .iter()
            .rposition(|line| !line.starts_with("-----") && !line.is_empty())
            .expect("armored output must contain payload lines");

        let mut bytes = lines[payload_idx].clone().into_bytes();
        bytes[0] = if bytes[0] == b'A' { b'B' } else { b'A' };
        lines[payload_idx] = String::from_utf8(bytes).unwrap();
        format!("{}\n", lines.join("\n"))
    }

    #[test]
    fn test_decrypt_wrong_passphrase_preserves_existing_plaintext() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("secret.txt");
        fs::write(&test_file, "encrypted-v1").unwrap();

        let passphrase = secret("correct");
        let encrypted_path = format!("{}.enc", test_file.to_str().unwrap());
        encrypt_file(test_file.to_str().unwrap(), &passphrase).unwrap();

        // Simulate local unencrypted modifications that must NOT be lost on wrong password
        let local_modified = "precious-local-plaintext-changes";
        fs::write(&test_file, local_modified).unwrap();

        let wrong_passphrase = secret("wrong");
        assert!(decrypt_file(&encrypted_path, &wrong_passphrase).is_err());
        assert_eq!(fs::read_to_string(&test_file).unwrap(), local_modified);
    }

    #[test]
    fn test_decrypt_tampered_payload_preserves_existing_and_leaves_no_partial() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("large_secret.txt");
        // > 64 KiB so age produces multiple STREAM chunks
        fs::write(&test_file, "A".repeat(128 * 1024)).unwrap();

        let passphrase = secret("pass123");
        let encrypted_path = temp_dir.path().join("large_secret.txt.enc");
        encrypt_file(test_file.to_str().unwrap(), &passphrase).unwrap();

        let armored = fs::read_to_string(&encrypted_path).unwrap();
        fs::write(&encrypted_path, corrupt_armored_tail(&armored)).unwrap();

        // Case 1: Plaintext does not exist -> must not leave partial unauthenticated output
        fs::remove_file(&test_file).unwrap();
        assert!(decrypt_file(encrypted_path.to_str().unwrap(), &passphrase).is_err());
        assert!(
            !test_file.exists(),
            "tampered multi-chunk ciphertext must not leave partial plaintext on disk"
        );

        // Case 2: Plaintext exists -> must preserve existing content
        fs::write(&test_file, "keep-me").unwrap();
        assert!(decrypt_file(encrypted_path.to_str().unwrap(), &passphrase).is_err());
        assert_eq!(fs::read_to_string(&test_file).unwrap(), "keep-me");
    }

    #[cfg(unix)]
    #[test]
    fn test_decrypt_enforces_0600_and_does_not_follow_symlink() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("secret.key");
        fs::write(&test_file, "private-key-material").unwrap();

        let passphrase = secret("secret");
        let encrypted_path = format!("{}.enc", test_file.to_str().unwrap());
        encrypt_file(test_file.to_str().unwrap(), &passphrase).unwrap();

        // Encrypted file should be 0600
        let enc_mode = fs::metadata(&encrypted_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(enc_mode, 0o600);

        // Make target world-readable 0644 before decrypt; atomic replace must tighten to 0600
        fs::set_permissions(&test_file, fs::Permissions::from_mode(0o644)).unwrap();
        decrypt_file(&encrypted_path, &passphrase).unwrap();
        let dec_mode = fs::metadata(&test_file).unwrap().permissions().mode() & 0o777;
        assert_eq!(dec_mode, 0o600);

        // Replace target with a symlink to victim file; decrypt must replace symlink, not victim
        let victim = temp_dir.path().join("victim.txt");
        fs::write(&victim, "untouched-victim").unwrap();
        fs::remove_file(&test_file).unwrap();
        symlink(&victim, &test_file).unwrap();

        decrypt_file(&encrypted_path, &passphrase).unwrap();
        assert_eq!(fs::read_to_string(&victim).unwrap(), "untouched-victim");
        assert!(
            !fs::symlink_metadata(&test_file)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read_to_string(&test_file).unwrap(),
            "private-key-material"
        );
    }

    #[test]
    fn test_decrypt_dotfile_and_reject_bare_enc_without_truncating() {
        let temp_dir = TempDir::new().unwrap();
        let passphrase = secret("pwd");

        // Hidden dotfile (.env -> .env.enc -> .env)
        let dotfile = temp_dir.path().join(".env");
        fs::write(&dotfile, "API_KEY=123").unwrap();
        encrypt_file(dotfile.to_str().unwrap(), &passphrase).unwrap();
        fs::remove_file(&dotfile).unwrap();
        let dotfile_enc = temp_dir.path().join(".env.enc");
        decrypt_file(dotfile_enc.to_str().unwrap(), &passphrase).unwrap();
        assert_eq!(fs::read_to_string(&dotfile).unwrap(), "API_KEY=123");

        // Bare ".enc" file must be rejected without truncating ".enc" itself
        let bare_enc = temp_dir.path().join(".enc");
        let original_bytes = fs::read(&dotfile_enc).unwrap();
        fs::write(&bare_enc, &original_bytes).unwrap();
        assert!(decrypt_file(bare_enc.to_str().unwrap(), &passphrase).is_err());
        assert_eq!(fs::read(&bare_enc).unwrap(), original_bytes);
    }
}
