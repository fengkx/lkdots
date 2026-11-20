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
        r#"registry=https://registry.npmjs.org/
@tencent:registry=https://mirrors.tencent.com/npm/
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
        r#"registry=
@tencent:registry=
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
    assert!(actual.contains("registry=https://registry.npmjs.org/"));
    assert!(actual.contains("@tencent:registry=https://mirrors.tencent.com/npm/"));
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
fn json_apply_keeps_target_bytes_when_managed_values_are_unchanged() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("codex-global-state.json");
    let target = temp.path().join(".codex-global-state.json");

    fs::write(
        &source,
        r#"{
  "git-commit-instructions": "Use concise commit messages."
}
"#,
    )
    .unwrap();
    let original =
        r#"{"git-commit-instructions":"Use concise commit messages.","other":"preserved"}"#;
    fs::write(&target, original).unwrap();

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
        .success();

    assert_eq!(fs::read_to_string(&target).unwrap(), original);
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
        r#"//registry.npmjs.org/:_authToken=npm_secret
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

#[test]
fn toml_apply_deep_merges_partial_preserves_comments_and_is_idempotent() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("config.partial.toml");
    let target = temp.path().join("config.toml");

    fs::write(
        &source,
        r#"model = "gpt-6.2-sol"

[features]
hooks = true
memories = true

[mcp_servers.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]
"#,
    )
    .unwrap();
    fs::write(
        &target,
        r#"# local header comment
model = "gpt-6.1-sol" # keep inline comment
base_url = "http://127.0.0.1:15721/v1"

[features]
hooks = false
js_repl = false

[projects."/Users/me/repo"]
trust_level = "trusted"
"#,
    )
    .unwrap();

    let config = write_config(
        &temp,
        &format!(
            r#"gitignore = "{}"

[[projections]]
name = "codex-config"
driver = "toml"
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
    assert!(actual.contains("# local header comment"));
    assert!(actual.contains(r#"model = "gpt-6.2-sol" # keep inline comment"#));
    assert!(actual.contains(r#"base_url = "http://127.0.0.1:15721/v1""#));
    assert!(actual.contains("hooks = true"));
    assert!(actual.contains("memories = true"));
    assert!(actual.contains("js_repl = false"));
    assert!(actual.contains(r#"[projects."/Users/me/repo"]"#));
    assert!(actual.contains("[mcp_servers.context7]"));
    assert!(actual.contains(r#"command = "npx""#));
}

#[test]
fn toml_apply_keeps_target_bytes_when_managed_values_are_unchanged() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("config.partial.toml");
    let target = temp.path().join("config.toml");

    fs::write(
        &source,
        r#"model = "gpt-6.2-sol"

[features]
hooks = true
"#,
    )
    .unwrap();
    let original = "# custom formatting\nmodel='gpt-6.2-sol'\n[features]\nhooks = true\nlocal = 1\n";
    fs::write(&target, original).unwrap();

    let config = write_config(
        &temp,
        &format!(
            r#"gitignore = "{}"

[[projections]]
name = "codex-config"
driver = "toml"
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
        .success();

    assert_eq!(fs::read_to_string(&target).unwrap(), original);
}

#[test]
fn toml_capture_updates_only_declared_keys_and_preserves_source_comments() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("config.partial.toml");
    let target = temp.path().join("config.toml");

    fs::write(
        &source,
        r#"# curated settings
model = "gpt-6.1-sol"

[features]
hooks = false
"#,
    )
    .unwrap();
    fs::write(
        &target,
        r#"model = "gpt-6.2-sol"
base_url = "http://127.0.0.1:15721/v1"

[features]
hooks = true
local_only = true

[projects."/Users/me/repo"]
trust_level = "trusted"
"#,
    )
    .unwrap();

    let config = write_config(
        &temp,
        &format!(
            r#"gitignore = "{}"

[[projections]]
name = "codex-config"
driver = "toml"
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
    assert!(actual.contains("# curated settings"));
    assert!(actual.contains(r#"model = "gpt-6.2-sol""#));
    assert!(actual.contains("hooks = true"));
    assert!(!actual.contains("base_url"));
    assert!(!actual.contains("local_only"));
    assert!(!actual.contains("projects"));
}

#[test]
fn toml_apply_rejects_non_table_intermediate_path() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("config.partial.toml");
    let target = temp.path().join("config.toml");

    fs::write(
        &source,
        r#"[features]
hooks = true
"#,
    )
    .unwrap();
    fs::write(&target, "features = \"unexpected scalar\"\n").unwrap();

    let config = write_config(
        &temp,
        &format!(
            r#"gitignore = "{}"

[[projections]]
name = "codex-config"
driver = "toml"
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
        .stderr(predicate::str::contains("Target TOML path must be a table"));
}

#[test]
fn toml_apply_and_capture_roundtrip_is_byte_stable_and_preserves_mode() {
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("config.partial.toml");
    let target = temp.path().join("config.toml");

    fs::write(
        &source,
        r#"# curated codex config
model = "gpt-6.2-sol"

[features]
hooks = true

[mcp_servers.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]
"#,
    )
    .unwrap();
    fs::write(
        &target,
        "# local target\nmodel = \"gpt-6.1-sol\"\n[projects.\"/Users/me/app\"]\ntrust_level = \"trusted\"",
    )
    .unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    }

    let config = write_config(
        &temp,
        &format!(
            r#"gitignore = "{}"

[[projections]]
name = "codex-config"
driver = "toml"
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
        .success();

    let after_first_apply = fs::read_to_string(&target).unwrap();
    let source_before_capture = fs::read_to_string(&source).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let meta1 = fs::metadata(&target).unwrap();
        assert_eq!(meta1.permissions().mode() & 0o777, 0o600);
        let ino1 = meta1.ino();

        // Second apply should be a complete no-op (same bytes, same inode)
        cargo_bin_cmd!("lkdots")
            .args(["-c", &config, "apply"])
            .assert()
            .success();

        let meta2 = fs::metadata(&target).unwrap();
        assert_eq!(meta2.ino(), ino1);
    }

    // Capture immediately after apply should be a complete no-op on source
    cargo_bin_cmd!("lkdots")
        .args(["-c", &config, "capture"])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(&source).unwrap(), source_before_capture);

    // Apply again after capture should keep exact bytes
    cargo_bin_cmd!("lkdots")
        .args(["-c", &config, "apply"])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(&target).unwrap(), after_first_apply);
}
