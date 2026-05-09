use crate::config::ProjectionDriver;
use anyhow::{Context, Result, anyhow};
use atomicwrites::{AllowOverwrite, AtomicFile};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::SystemTime,
};
use toml::Value as TomlValue;

#[derive(Debug, Clone)]
pub struct Projection {
    pub name: String,
    pub driver: ProjectionDriver,
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, Copy)]
pub enum ProjectionAction {
    Apply,
    Capture,
}

pub fn run_all(
    projections: &[Projection],
    base_dir: &Path,
    action: ProjectionAction,
    simulate: bool,
) -> Result<()> {
    for projection in projections {
        projection.run(base_dir, action, simulate)?;
    }
    Ok(())
}

impl Projection {
    fn run(&self, base_dir: &Path, action: ProjectionAction, simulate: bool) -> Result<()> {
        match (&self.driver, action) {
            (ProjectionDriver::Properties, ProjectionAction::Apply) => {
                apply_properties(self, base_dir, simulate)
            }
            (ProjectionDriver::Properties, ProjectionAction::Capture) => {
                capture_properties(self, base_dir, simulate)
            }
            (ProjectionDriver::Json, ProjectionAction::Apply) => {
                apply_json(self, base_dir, simulate)
            }
            (ProjectionDriver::Json, ProjectionAction::Capture) => {
                capture_json(self, base_dir, simulate)
            }
        }
    }
}

fn expand_path(path: &str, base_dir: &Path) -> PathBuf {
    let expanded = shellexpand::tilde(path);
    let path = Path::new(expanded.as_ref());
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    }
}

fn read_to_string_if_exists(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("Failed to read {}", path.display())),
    }
}

fn atomic_write_if_changed(
    path: &Path,
    expected_metadata: Option<FileSnapshot>,
    content: &str,
    simulate: bool,
) -> Result<()> {
    let current = read_to_string_if_exists(path)?;
    if current.as_deref() == Some(content) {
        return Ok(());
    }

    if simulate {
        println!("projection would write {}", path.display());
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    if let Some(expected) = expected_metadata {
        let current = FileSnapshot::read(path)?;
        if current != Some(expected) {
            return Err(anyhow!(
                "Target changed while applying projection: {}",
                path.display()
            ));
        }
    }

    let file = AtomicFile::new(path, AllowOverwrite);
    file.write(|writer| writer.write_all(content.as_bytes()))
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileSnapshot {
    len: u64,
    modified: Option<SystemTime>,
}

impl FileSnapshot {
    fn read(path: &Path) -> Result<Option<Self>> {
        match fs::metadata(path) {
            Ok(metadata) => Ok(Some(Self {
                len: metadata.len(),
                modified: metadata.modified().ok(),
            })),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err).with_context(|| format!("Failed to stat {}", path.display())),
        }
    }
}

fn apply_properties(projection: &Projection, base_dir: &Path, simulate: bool) -> Result<()> {
    let source_path = expand_path(&projection.source, base_dir);
    let target_path = expand_path(&projection.target, base_dir);
    let desired = read_properties_source(&source_path)?;
    reject_secret_keys(desired.keys())?;

    let before = FileSnapshot::read(&target_path)?;
    let target = read_to_string_if_exists(&target_path)?.unwrap_or_default();
    let next = merge_properties_target(&target, &desired);
    atomic_write_if_changed(&target_path, before, &next, simulate)
}

fn capture_properties(projection: &Projection, base_dir: &Path, simulate: bool) -> Result<()> {
    let source_path = expand_path(&projection.source, base_dir);
    let target_path = expand_path(&projection.target, base_dir);
    let declared = read_properties_source(&source_path)?;
    reject_secret_keys(declared.keys())?;
    let wanted: BTreeSet<String> = declared.keys().cloned().collect();
    let target = fs::read_to_string(&target_path)
        .with_context(|| format!("Failed to read {}", target_path.display()))?;
    let captured = capture_properties_values(&target, &wanted)?;
    let next = properties_source_to_toml(&captured)?;
    atomic_write_if_changed(
        &source_path,
        FileSnapshot::read(&source_path)?,
        &next,
        simulate,
    )
}

fn read_properties_source(path: &Path) -> Result<BTreeMap<String, String>> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let value: TomlValue = toml::from_str(&content)
        .with_context(|| format!("Failed to parse properties source {}", path.display()))?;
    let values = value
        .get("values")
        .and_then(TomlValue::as_table)
        .ok_or_else(|| anyhow!("Properties projection source must contain a [values] table"))?;

    let mut result = BTreeMap::new();
    for (key, value) in values {
        let value = value
            .as_str()
            .ok_or_else(|| anyhow!("Properties value for key '{}' must be a string", key))?;
        result.insert(key.to_string(), value.to_string());
    }
    Ok(result)
}

fn properties_source_to_toml(values: &BTreeMap<String, String>) -> Result<String> {
    let mut table = toml::map::Map::new();
    let mut value_table = toml::map::Map::new();
    for (key, value) in values {
        value_table.insert(key.clone(), TomlValue::String(value.clone()));
    }
    table.insert("values".to_string(), TomlValue::Table(value_table));
    let mut content = toml::to_string_pretty(&TomlValue::Table(table))?;
    if !content.ends_with('\n') {
        content.push('\n');
    }
    Ok(content)
}

fn reject_secret_keys<'a>(keys: impl IntoIterator<Item = &'a String>) -> Result<()> {
    for key in keys {
        let lower = key.to_ascii_lowercase();
        if lower.contains("token")
            || lower.contains("password")
            || lower.contains("_authtoken")
            || lower.contains("_password")
        {
            return Err(anyhow!(
                "Projection refuses to manage secret-like key '{}'",
                key
            ));
        }
    }
    Ok(())
}

fn parse_property_line(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
        return None;
    }
    let (key, _) = trimmed.split_once('=')?;
    let key = key.trim();
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

fn merge_properties_target(target: &str, desired: &BTreeMap<String, String>) -> String {
    let mut seen = HashSet::new();
    let mut output = Vec::new();

    for line in target.lines() {
        if let Some(key) = parse_property_line(line) {
            if let Some(value) = desired.get(&key) {
                if seen.insert(key.clone()) {
                    output.push(format!("{}={}", key, value));
                }
                continue;
            }
        }
        output.push(line.to_string());
    }

    for (key, value) in desired {
        if !seen.contains(key) {
            output.push(format!("{}={}", key, value));
        }
    }

    let mut content = output.join("\n");
    if !content.is_empty() {
        content.push('\n');
    }
    content
}

fn capture_properties_values(
    target: &str,
    wanted: &BTreeSet<String>,
) -> Result<BTreeMap<String, String>> {
    let mut captured = BTreeMap::new();
    for line in target.lines() {
        let trimmed = line.trim_start();
        let Some((raw_key, raw_value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = raw_key.trim();
        if wanted.contains(key) && !captured.contains_key(key) {
            captured.insert(key.to_string(), raw_value.trim().to_string());
        }
    }

    for key in wanted {
        if !captured.contains_key(key) {
            return Err(anyhow!("Projection key '{}' not found in target", key));
        }
    }

    Ok(captured)
}

fn apply_json(projection: &Projection, base_dir: &Path, simulate: bool) -> Result<()> {
    let source_path = expand_path(&projection.source, base_dir);
    let target_path = expand_path(&projection.target, base_dir);
    let source = read_json_file(&source_path)?;
    let before = FileSnapshot::read(&target_path)?;
    let mut target = match read_to_string_if_exists(&target_path)? {
        Some(content) => serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", target_path.display()))?,
        None => JsonValue::Object(JsonMap::new()),
    };

    merge_json(&mut target, &source)?;
    let next = format!("{}\n", serde_json::to_string_pretty(&target)?);
    atomic_write_if_changed(&target_path, before, &next, simulate)
}

fn capture_json(projection: &Projection, base_dir: &Path, simulate: bool) -> Result<()> {
    let source_path = expand_path(&projection.source, base_dir);
    let target_path = expand_path(&projection.target, base_dir);
    let source = read_json_file(&source_path)?;
    let target = read_json_file(&target_path)?;
    let paths = collect_json_leaf_paths(&source);
    let mut captured = JsonValue::Object(JsonMap::new());

    for path in paths {
        let value = get_json_path(&target, &path)
            .ok_or_else(|| anyhow!("Projection path '{}' not found in target", path.join(".")))?;
        set_json_path(&mut captured, &path, value.clone())?;
    }

    let next = format!("{}\n", serde_json::to_string_pretty(&captured)?);
    atomic_write_if_changed(
        &source_path,
        FileSnapshot::read(&source_path)?,
        &next,
        simulate,
    )
}

fn read_json_file(path: &Path) -> Result<JsonValue> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    serde_json::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))
}

fn merge_json(target: &mut JsonValue, source: &JsonValue) -> Result<()> {
    match (target, source) {
        (JsonValue::Object(target_object), JsonValue::Object(source_object)) => {
            for (key, source_value) in source_object {
                if let Some(target_value) = target_object.get_mut(key) {
                    merge_json(target_value, source_value)
                        .with_context(|| format!("Failed to merge JSON object at key '{}'", key))?;
                } else {
                    target_object.insert(key.clone(), source_value.clone());
                }
            }
            Ok(())
        }
        (_, JsonValue::Object(_)) => Err(anyhow!("Target JSON path must be an object")),
        (target_value, source_value) => {
            *target_value = source_value.clone();
            Ok(())
        }
    }
}

fn collect_json_leaf_paths(value: &JsonValue) -> Vec<Vec<String>> {
    fn walk(value: &JsonValue, prefix: &mut Vec<String>, paths: &mut Vec<Vec<String>>) {
        match value {
            JsonValue::Object(object) if !object.is_empty() => {
                for (key, child) in object {
                    prefix.push(key.clone());
                    walk(child, prefix, paths);
                    prefix.pop();
                }
            }
            _ => paths.push(prefix.clone()),
        }
    }

    let mut paths = Vec::new();
    walk(value, &mut Vec::new(), &mut paths);
    paths
}

fn get_json_path<'a>(value: &'a JsonValue, path: &[String]) -> Option<&'a JsonValue> {
    let mut current = value;
    for segment in path {
        current = current.as_object()?.get(segment)?;
    }
    Some(current)
}

fn set_json_path(target: &mut JsonValue, path: &[String], value: JsonValue) -> Result<()> {
    if path.is_empty() {
        *target = value;
        return Ok(());
    }

    let mut current = target;
    for segment in &path[..path.len() - 1] {
        if !current.is_object() {
            *current = JsonValue::Object(JsonMap::new());
        }
        let object = current
            .as_object_mut()
            .ok_or_else(|| anyhow!("Failed to create JSON object path"))?;
        current = object
            .entry(segment.clone())
            .or_insert_with(|| JsonValue::Object(JsonMap::new()));
    }

    if !current.is_object() {
        *current = JsonValue::Object(JsonMap::new());
    }
    let object = current
        .as_object_mut()
        .ok_or_else(|| anyhow!("Failed to create JSON object path"))?;
    object.insert(path[path.len() - 1].clone(), value);
    Ok(())
}
