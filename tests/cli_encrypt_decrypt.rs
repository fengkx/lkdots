use assert_cmd::Command;
use predicates::str::contains;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn write_config(base: &TempDir, from: &PathBuf, gitignore: &PathBuf) -> PathBuf {
    let cfg_path = base.path().join("lkdots.toml");
    let toml = format!(
        r#"
gitignore = "{gitignore}"

[[entries]]
from = "{from}"
to = "~/tmp-dot-target"
platforms = ["linux", "darwin"]
encrypt = true
"#,
        from = from.display(),
        gitignore = gitignore.display()
    );
    fs::write(&cfg_path, toml).expect("write config");
    cfg_path
}

#[test]
fn encrypt_and_decrypt_roundtrip_with_temp_files() {
    let tmp = TempDir::new().unwrap();
    let source_dir = tmp.path().join("source");
    fs::create_dir(&source_dir).unwrap();
    let source = source_dir.join("plain.txt");
    fs::write(&source, "hello-world").unwrap();

    let gitignore = tmp.path().join(".gitignore");
    fs::write(&gitignore, "").unwrap();

    let cfg = write_config(&tmp, &source_dir, &gitignore);

    // Encrypt with env-provided passphrase
    Command::cargo_bin("lkdots")
        .unwrap()
        .env("LKDOTS_PASSPHRASE", "pwd")
        .args(["-c", cfg.to_str().unwrap(), "encrypt"])
        .assert()
        .success()
        .stdout(contains("Successfully encrypted"));

    let enc_path = source_dir.join("plain.txt.enc");
    assert!(enc_path.exists(), "encrypted file should exist");

    // Decrypt with env-provided passphrase
    Command::cargo_bin("lkdots")
        .unwrap()
        .env("LKDOTS_PASSPHRASE", "pwd")
        .args(["-c", cfg.to_str().unwrap(), "decrypt"])
        .assert()
        .success()
        .stdout(contains("Successfully decrypted"));

    let decrypted = fs::read_to_string(&source).unwrap();
    assert_eq!(decrypted, "hello-world");
}
