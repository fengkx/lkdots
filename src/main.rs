mod cli;
mod config;
mod crypto;
mod gitignore;
mod operations;
mod output;
mod path_util;
mod symlink_util;

use age::secrecy::{ExposeSecret, SecretString};
use anyhow::{Context, Result, anyhow};
use config::ConfigFileStruct;
use indicatif::{ProgressBar, ProgressStyle};
use log::debug;
use path_util::get_dir;
use rayon::prelude::*;
use rpassword::prompt_password;
use std::{
    fs::read_to_string,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::Arc,
};
use subtle::ConstantTimeEq;
use walkdir::WalkDir;
use zeroize::Zeroize;

use crate::{
    config::Config,
    crypto::{decrypt_file, encrypt_file},
    operations::{Op, execute},
    output::{print_info, print_success},
};
use colored::*;

fn main() -> Result<()> {
    env_logger::init();

    let cfg = cli::config()?;
    let cfg_str = read_to_string(&cfg.config);
    if let Err(err) = cfg_str {
        debug!("{}", err);
        if err.kind() == ErrorKind::NotFound {
            return Err(anyhow!(
                "Config file not found: {}\n\n\
                Hint: Use -c option to specify config file path\n\
                Default: lkdots.toml in current directory",
                cfg.config
            ));
        }
        return Err(anyhow!(err));
    }
    let config: Config = toml::from_str::<ConfigFileStruct>(&cfg_str?)?.into();

    // Validate configuration
    config
        .validate()
        .context("Configuration validation failed")?;

    let base_dir = get_dir(Path::new(&cfg.config))?;
    let entries = &config.entries;

    if cfg.is_encrypt_cmd() || cfg.is_decrypt_cmd() {
        let env_pass = std::env::var("LKDOTS_PASSPHRASE").ok();
        let phrase = if let Some(p) = env_pass.clone() {
            SecretString::from(p)
        } else {
            SecretString::from(prompt_password("Passphrase: ")?)
        };
        if cfg.is_encrypt_cmd() && env_pass.is_none() {
            let mut again_phrase = prompt_password("Input passphrase again: ")?;
            if !constant_time_eq(phrase.expose_secret(), &again_phrase) {
                again_phrase.zeroize();
                return Err(anyhow!("Passphrase verification failed"));
            }
            again_phrase.zeroize();
        }

        // Phase 1: Collect files to process
        let files = collect_files_to_process(entries, cfg.is_encrypt_cmd())?;

        if files.is_empty() {
            print_info("No files to process.");
            return Ok(());
        }

        // Phase 2: Process files in parallel (with progress bar)
        let phrase_arc = Arc::new(phrase);
        let result = if cfg.is_encrypt_cmd() {
            encrypt_files_parallel(files, phrase_arc)
        } else {
            decrypt_files_parallel(files, phrase_arc)
        };

        return result;
    }

    let r = entries
        .par_iter()
        .filter(|e| e.match_platform())
        .map(|cfg| cfg.create_ops(base_dir));
    let opss = r
        .collect::<Result<Vec<Vec<Op>>>>()
        .context("Failed to create operations for some entries")?;

    if cfg.simulate {
        println!(
            "{}",
            "Simulation Mode - No changes will be made"
                .bold()
                .underline()
        );
        println!();
        for ops in &opss {
            for op in ops {
                match op {
                    Op::Mkdirp(p) => println!("{} {}", "→".blue(), p.cyan()),
                    Op::Symlink(from, to, _) => {
                        println!("{} {} → {}", "→".green(), to.cyan(), from);
                    }
                    Op::Existed(p) => println!("{} {} (already exists)", "•".dimmed(), p.dimmed()),
                    Op::Conflict(p) => println!("{} {} (CONFLICT)", "✗".red(), p.red()),
                }
            }
        }
    } else {
        opss.par_iter()
            .map(|ops| -> Result<()> { execute(ops) })
            .collect::<Result<()>>()?;
    }
    crate::gitignore::write_gitignore(&config, cfg.simulate)?;
    Ok(())
}

/// Constant-time string comparison to prevent timing attacks
/// Even if passwords don't match, the full comparison operation is performed
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }

    // Use subtle library for constant-time comparison
    a.as_bytes().ct_eq(b.as_bytes()).unwrap_u8() == 1
}

/// Collect list of files that need to be encrypted or decrypted
fn collect_files_to_process(
    entries: &[crate::config::Entry],
    is_encrypt: bool,
) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for entry in entries.iter().filter(|e| e.encrypt) {
        let expanded_from = shellexpand::tilde(entry.from.as_ref());
        let walker = WalkDir::new(expanded_from.as_ref())
            .follow_links(false)
            .into_iter();

        for entry_result in walker.filter_entry(|e| !e.path_is_symlink()) {
            let entry = entry_result?;
            if entry.metadata()?.is_file() {
                let path = entry.path();
                let path_str = path.to_string_lossy();

                if is_encrypt {
                    // Encryption: skip already encrypted files
                    if !path_str.ends_with(".enc") {
                        files.push(path.to_path_buf());
                    }
                } else {
                    // Decryption: only process .enc files
                    if path_str.ends_with(".enc") {
                        files.push(path.to_path_buf());
                    }
                }
            }
        }
    }

    Ok(files)
}

/// Truncate path string safely for UTF-8 characters
fn truncate_path(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_len {
        s.to_string()
    } else {
        let suffix: String = s
            .chars()
            .rev()
            .take(max_len - 3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("...{}", suffix)
    }
}

/// Shared parallel file processor with progress bar
fn process_files_parallel<F>(
    files: Vec<PathBuf>,
    passphrase: Arc<SecretString>,
    op: F,
    kind_display: &str,
    success_verb: &str,
) -> Result<()>
where
    F: Fn(&str, &SecretString) -> Result<()> + Sync + Send,
{
    let total = files.len();
    if total == 0 {
        return Ok(());
    }

    // Create progress bar
    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} files ({percent}%) {msg}")
            .unwrap()
            .progress_chars("#>-"),
    );

    // Process files in parallel (ProgressBar is thread-safe and can be cloned directly)
    files.par_iter().try_for_each(|file_path| -> Result<()> {
        let file_str = file_path.to_string_lossy();
        let pb = pb.clone();
        let passphrase = Arc::clone(&passphrase);

        // Update progress bar message
        let display_name = truncate_path(&file_str, 50);
        pb.set_message(display_name);

        // Execute operation
        op(&file_str, passphrase.as_ref())?;
        pb.inc(1);
        Ok(())
    })?;

    pb.finish_with_message(format!("{} completed", kind_display));
    print_success(&format!("Successfully {} {} file(s)", success_verb, total));

    Ok(())
}

/// Encrypt files in parallel with progress bar
fn encrypt_files_parallel(files: Vec<PathBuf>, passphrase: Arc<SecretString>) -> Result<()> {
    process_files_parallel(files, passphrase, encrypt_file, "Encryption", "encrypted")
}

/// Decrypt files in parallel with progress bar
fn decrypt_files_parallel(files: Vec<PathBuf>, passphrase: Arc<SecretString>) -> Result<()> {
    process_files_parallel(files, passphrase, decrypt_file, "Decryption", "decrypted")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "def"));
        assert!(!constant_time_eq("abc", "abcd"));
        assert!(!constant_time_eq("abcd", "abc"));
        assert!(constant_time_eq("", ""));
        assert!(!constant_time_eq("a", ""));
    }

    #[test]
    fn test_truncate_path_short() {
        let path = "short";
        let result = truncate_path(path, 10);
        assert_eq!(result, "short");
    }

    #[test]
    fn test_truncate_path_long() {
        let path = "this/is/a/very/long/path/that/needs/truncation";
        let result = truncate_path(path, 20);
        assert!(result.len() <= 20);
        assert!(result.starts_with("..."));
    }

    #[test]
    fn test_truncate_path_exact_length() {
        let path = "exact";
        let result = truncate_path(path, 5);
        assert_eq!(result, "exact");
    }

    #[test]
    fn test_truncate_path_unicode() {
        let path = "测试/路径/中文";
        let result = truncate_path(path, 10);
        // Should handle UTF-8 correctly
        assert!(result.chars().count() <= 10);
    }

    #[test]
    fn test_collect_files_to_process_encrypt_mode() {
        use crate::config::{Entry, Platform};
        use std::borrow::Cow;
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        fs::write(&test_file, "content").unwrap();

        let encrypted_file = temp_dir.path().join("test.enc");
        fs::write(&encrypted_file, "encrypted content").unwrap();

        let entries = vec![Entry {
            from: Cow::Owned(temp_dir.path().to_str().unwrap().to_string()),
            to: Cow::Owned("~/test".to_string()),
            platforms: Cow::Owned(vec![Platform::Linux, Platform::Darwin]),
            encrypt: true,
        }];

        // Encrypt mode: should find test.txt but skip test.enc
        let files = collect_files_to_process(&entries, true).unwrap();
        assert!(
            files
                .iter()
                .any(|f| f.to_string_lossy().contains("test.txt"))
        );
        assert!(!files.iter().any(|f| f.to_string_lossy().ends_with(".enc")));
    }

    #[test]
    fn test_collect_files_to_process_decrypt_mode() {
        use crate::config::{Entry, Platform};
        use std::borrow::Cow;
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        fs::write(&test_file, "content").unwrap();

        let encrypted_file = temp_dir.path().join("test.enc");
        fs::write(&encrypted_file, "encrypted content").unwrap();

        let entries = vec![Entry {
            from: Cow::Owned(temp_dir.path().to_str().unwrap().to_string()),
            to: Cow::Owned("~/test".to_string()),
            platforms: Cow::Owned(vec![Platform::Linux, Platform::Darwin]),
            encrypt: true,
        }];

        // Decrypt mode: should find test.enc but skip test.txt
        let files = collect_files_to_process(&entries, false).unwrap();
        assert!(files.iter().any(|f| f.to_string_lossy().ends_with(".enc")));
        assert!(
            !files
                .iter()
                .any(|f| f.to_string_lossy().contains("test.txt")
                    && !f.to_string_lossy().ends_with(".enc"))
        );
    }

    #[test]
    fn test_collect_files_to_process_no_encrypt_entries() {
        use crate::config::{Entry, Platform};
        use std::borrow::Cow;
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        fs::write(&test_file, "content").unwrap();

        let entries = vec![Entry {
            from: Cow::Owned(temp_dir.path().to_str().unwrap().to_string()),
            to: Cow::Owned("~/test".to_string()),
            platforms: Cow::Owned(vec![Platform::Linux, Platform::Darwin]),
            encrypt: false, // encrypt is false
        }];

        // No encrypt entries: should return empty
        let files = collect_files_to_process(&entries, true).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn test_collect_files_to_process_nested_dirs() {
        use crate::config::{Entry, Platform};
        use std::borrow::Cow;
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let subdir = temp_dir.path().join("subdir");
        fs::create_dir(&subdir).unwrap();

        let test_file1 = temp_dir.path().join("test1.txt");
        fs::write(&test_file1, "content1").unwrap();

        let test_file2 = subdir.join("test2.txt");
        fs::write(&test_file2, "content2").unwrap();

        let entries = vec![Entry {
            from: Cow::Owned(temp_dir.path().to_str().unwrap().to_string()),
            to: Cow::Owned("~/test".to_string()),
            platforms: Cow::Owned(vec![Platform::Linux, Platform::Darwin]),
            encrypt: true,
        }];

        // Should find files in nested directories
        let files = collect_files_to_process(&entries, true).unwrap();
        assert!(files.len() >= 2);
    }

    #[test]
    fn test_encrypt_files_parallel_empty() {
        use age::secrecy::SecretString;
        use std::sync::Arc;

        let files = Vec::new();
        let passphrase = Arc::new(SecretString::new("test".to_string().into_boxed_str()));

        // Should return Ok for empty files
        let result = encrypt_files_parallel(files, passphrase);
        assert!(result.is_ok());
    }

    #[test]
    fn test_decrypt_files_parallel_empty() {
        use age::secrecy::SecretString;
        use std::sync::Arc;

        let files = Vec::new();
        let passphrase = Arc::new(SecretString::new("test".to_string().into_boxed_str()));

        // Should return Ok for empty files
        let result = decrypt_files_parallel(files, passphrase);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_files_parallel_success() {
        use age::secrecy::SecretString;
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let f1 = temp_dir.path().join("a.txt");
        let f2 = temp_dir.path().join("b.txt");
        std::fs::write(&f1, "1").unwrap();
        std::fs::write(&f2, "2").unwrap();

        let counter = Arc::new(AtomicUsize::new(0));
        let passphrase = Arc::new(SecretString::new("pwd".to_string().into_boxed_str()));

        let op_counter = Arc::clone(&counter);
        let result = process_files_parallel(
            vec![f1, f2],
            Arc::clone(&passphrase),
            move |path, secret| {
                assert_eq!(secret.expose_secret(), "pwd");
                assert!(std::path::Path::new(path).exists());
                op_counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
            "Test",
            "tested",
        );

        assert!(result.is_ok());
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_process_files_parallel_failure() {
        use age::secrecy::SecretString;
        use std::sync::Arc;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let f1 = temp_dir.path().join("ok.txt");
        let f2 = temp_dir.path().join("fail.txt");
        std::fs::write(&f1, "ok").unwrap();
        std::fs::write(&f2, "fail").unwrap();

        let passphrase = Arc::new(SecretString::new("pwd".to_string().into_boxed_str()));

        let result = process_files_parallel(
            vec![f1, f2],
            passphrase,
            |path, _| {
                if path.contains("fail") {
                    Err(anyhow::anyhow!("expected failure"))
                } else {
                    Ok(())
                }
            },
            "Test",
            "tested",
        );

        assert!(result.is_err());
    }
}
