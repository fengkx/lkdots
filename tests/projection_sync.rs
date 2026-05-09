use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn write_config(temp: &TempDir, body: &str) -> String {
    let config = temp.path().join("lkdots.toml");
    fs::write(&config, body).unwrap();
    config.to_string_lossy().to_string()
}

#[test]
fn properties_apply_is_idempotent_and_preserves_unmanaged_values() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("npmrc.toml");
    let target = temp.path().join(".npmrc");

    fs::write(
        &source,
        r#"[values]
registry = "https://registry.npmjs.org/"
"@tencent:registry" = "https://mirrors.tencent.com/npm/"
"#,
    )
    .unwrap();
    fs::write(
        &target,
        r#"# local npm config
registry=https://old.invalid/
//registry.npmjs.org/:_authToken=npm_secret
registry=https://duplicate.invalid/
"#,
    )
    .unwrap();

    let config = write_config(
        &temp,
        &format!(
            r#"gitignore = "{}"

[[projections]]
name = "npmrc"
driver = "properties"
source = "{}"
target = "{}"
"#,
            temp.path().join(".gitignore").display(),
            source.display(),
            target.display()
        ),
    );

    for _ in 0..2 {
        cargo_bin_cmd!("lkdots")
            .args(["-c", &config, "apply"])
            .assert()
            .success();
    }

    let actual = fs::read_to_string(&target).unwrap();
    assert_eq!(
        actual
            .lines()
            .filter(|line| line.starts_with("registry="))
            .count(),
        1
    );
    assert!(actual.contains("registry=https://registry.npmjs.org/"));
    assert!(actual.contains("@tencent:registry=https://mirrors.tencent.com/npm/"));
    assert!(actual.contains("//registry.npmjs.org/:_authToken=npm_secret"));
    assert!(!actual.contains("duplicate.invalid"));
}

#[test]
fn properties_capture_writes_only_declared_values() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("npmrc.toml");
    let target = temp.path().join(".npmrc");

    fs::write(
        &source,
        r#"[values]
registry = ""
"@tencent:registry" = ""
"#,
    )
    .unwrap();
    fs::write(
        &target,
        r#"registry=https://registry.npmjs.org/
@tencent:registry=https://mirrors.tencent.com/npm/
//registry.npmjs.org/:_authToken=npm_secret
email=local@example.test
"#,
    )
    .unwrap();

    let config = write_config(
        &temp,
        &format!(
            r#"gitignore = "{}"

[[projections]]
name = "npmrc"
driver = "properties"
source = "{}"
target = "{}"
"#,
            temp.path().join(".gitignore").display(),
            source.display(),
            target.display()
        ),
    );

    cargo_bin_cmd!("lkdots")
        .args(["-c", &config, "capture"])
        .assert()
        .success();

    let actual = fs::read_to_string(&source).unwrap();
    assert!(actual.contains(r#"registry = "https://registry.npmjs.org/""#));
    assert!(actual.contains(r#""@tencent:registry" = "https://mirrors.tencent.com/npm/""#));
    assert!(!actual.contains("authToken"));
    assert!(!actual.contains("email"));
}

#[test]
fn json_apply_deep_merges_partial_and_is_idempotent() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("codex-global-state.json");
    let target = temp.path().join(".codex-global-state.json");

    fs::write(
        &source,
        r#"{
  "electron-persisted-atom-state": {
    "git-commit-instructions": "Use concise commit messages.",
    "nested": {
      "enabled": true
    }
  }
}
"#,
    )
    .unwrap();
    fs::write(
        &target,
        r#"{
  "electron-persisted-atom-state": {
    "git-commit-instructions": "old",
    "other": "preserved"
  },
  "top-level": 42
}
"#,
    )
    .unwrap();

    let config = write_config(
        &temp,
        &format!(
            r#"gitignore = "{}"

[[projections]]
name = "codex-global-state"
driver = "json"
source = "{}"
target = "{}"
"#,
            temp.path().join(".gitignore").display(),
            source.display(),
            target.display()
        ),
    );

    for _ in 0..2 {
        cargo_bin_cmd!("lkdots")
            .args(["-c", &config, "apply"])
            .assert()
            .success();
    }

    let actual = fs::read_to_string(&target).unwrap();
    assert!(actual.contains(r#""git-commit-instructions": "Use concise commit messages.""#));
    assert!(actual.contains(r#""enabled": true"#));
    assert!(actual.contains(r#""other": "preserved""#));
    assert!(actual.contains(r#""top-level": 42"#));
}

#[test]
fn json_capture_extracts_source_leaf_paths() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("codex-global-state.json");
    let target = temp.path().join(".codex-global-state.json");

    fs::write(
        &source,
        r#"{
  "electron-persisted-atom-state": {
    "git-commit-instructions": "",
    "nested": {
      "enabled": false
    }
  }
}
"#,
    )
    .unwrap();
    fs::write(
        &target,
        r#"{
  "electron-persisted-atom-state": {
    "git-commit-instructions": "Captured text",
    "nested": {
      "enabled": true,
      "ignored": "not captured"
    },
    "other": "not captured"
  }
}
"#,
    )
    .unwrap();

    let config = write_config(
        &temp,
        &format!(
            r#"gitignore = "{}"

[[projections]]
name = "codex-global-state"
driver = "json"
source = "{}"
target = "{}"
"#,
            temp.path().join(".gitignore").display(),
            source.display(),
            target.display()
        ),
    );

    cargo_bin_cmd!("lkdots")
        .args(["-c", &config, "capture"])
        .assert()
        .success();

    let actual = fs::read_to_string(&source).unwrap();
    assert!(actual.contains(r#""git-commit-instructions": "Captured text""#));
    assert!(actual.contains(r#""enabled": true"#));
    assert!(!actual.contains("ignored"));
    assert!(!actual.contains("other"));
}

#[test]
fn json_apply_rejects_non_object_intermediate_path() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("codex-global-state.json");
    let target = temp.path().join(".codex-global-state.json");

    fs::write(
        &source,
        r#"{
  "electron-persisted-atom-state": {
    "git-commit-instructions": "Use concise commit messages."
  }
}
"#,
    )
    .unwrap();
    fs::write(
        &target,
        r#"{
  "electron-persisted-atom-state": "unexpected scalar"
}
"#,
    )
    .unwrap();

    let config = write_config(
        &temp,
        &format!(
            r#"gitignore = "{}"

[[projections]]
name = "codex-global-state"
driver = "json"
source = "{}"
target = "{}"
"#,
            temp.path().join(".gitignore").display(),
            source.display(),
            target.display()
        ),
    );

    cargo_bin_cmd!("lkdots")
        .args(["-c", &config, "apply"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Target JSON path must be an object",
        ));
}

#[test]
fn properties_apply_rejects_secret_source_keys() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("npmrc.toml");
    let target = temp.path().join(".npmrc");

    fs::write(
        &source,
        r#"[values]
"//registry.npmjs.org/:_authToken" = "npm_secret"
"#,
    )
    .unwrap();
    fs::write(&target, "").unwrap();

    let config = write_config(
        &temp,
        &format!(
            r#"gitignore = "{}"

[[projections]]
name = "npmrc"
driver = "properties"
source = "{}"
target = "{}"
"#,
            temp.path().join(".gitignore").display(),
            source.display(),
            target.display()
        ),
    );

    cargo_bin_cmd!("lkdots")
        .args(["-c", &config, "apply"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "refuses to manage secret-like key",
        ));
}
