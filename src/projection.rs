use crate::config::ProjectionDriver;
use crate::path_util::{FileModePolicy, atomic_write};
use anyhow::{Context, Result, anyhow};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::SystemTime,
};
use toml_edit::{DocumentMut, Item as TomlItem, TableLike as TomlTableLike, Value as TomlValue};

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
            (ProjectionDriver::Toml, ProjectionAction::Apply) => {
                apply_toml(self, base_dir, simulate)
            }
            (ProjectionDriver::Toml, ProjectionAction::Capture) => {
                capture_toml(self, base_dir, simulate)
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

    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
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

    atomic_write(path, FileModePolicy::PreserveExisting, |writer| {
        writer.write_all(content.as_bytes())
    })
    .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileSnapshot {
    len: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    ino: u64,
}

impl FileSnapshot {
    fn read(path: &Path) -> Result<Option<Self>> {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;

        match fs::metadata(path) {
            Ok(metadata) => Ok(Some(Self {
                len: metadata.len(),
                modified: metadata.modified().ok(),
                #[cfg(unix)]
                ino: metadata.ino(),
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
    let next = properties_source_to_text(&captured);
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
    let mut result = BTreeMap::new();
    for (line_number, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }

        let Some((raw_key, raw_value)) = trimmed.split_once('=') else {
            return Err(anyhow!(
                "Invalid properties source line {} in {}: expected key=value",
                line_number + 1,
                path.display()
            ));
        };
        let key = raw_key.trim();
        if key.is_empty() {
            return Err(anyhow!(
                "Invalid properties source line {} in {}: key is empty",
                line_number + 1,
                path.display()
            ));
        }
        result.insert(key.to_string(), raw_value.trim().to_string());
    }
    Ok(result)
}

fn properties_source_to_text(values: &BTreeMap<String, String>) -> String {
    let mut content = values
        .iter()
        .map(|(key, value)| format!("{}={}", key, value))
        .collect::<Vec<_>>()
        .join("\n");
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content
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

    if !merge_json(&mut target, &source)? {
        return Ok(());
    }
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

fn merge_json(target: &mut JsonValue, source: &JsonValue) -> Result<bool> {
    match (target, source) {
        (JsonValue::Object(target_object), JsonValue::Object(source_object)) => {
            let mut changed = false;
            for (key, source_value) in source_object {
                if let Some(target_value) = target_object.get_mut(key) {
                    changed |= merge_json(target_value, source_value)
                        .with_context(|| format!("Failed to merge JSON object at key '{}'", key))?;
                } else {
                    target_object.insert(key.clone(), source_value.clone());
                    changed = true;
                }
            }
            Ok(changed)
        }
        (_, JsonValue::Object(_)) => Err(anyhow!("Target JSON path must be an object")),
        (target_value, source_value) => {
            if target_value == source_value {
                Ok(false)
            } else {
                *target_value = source_value.clone();
                Ok(true)
            }
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

fn apply_toml(projection: &Projection, base_dir: &Path, simulate: bool) -> Result<()> {
    let source_path = expand_path(&projection.source, base_dir);
    let target_path = expand_path(&projection.target, base_dir);
    let source = read_toml_document(&source_path)?;
    let before = FileSnapshot::read(&target_path)?;
    let mut target = match read_to_string_if_exists(&target_path)? {
        Some(content) => content
            .parse::<DocumentMut>()
            .with_context(|| format!("Failed to parse {}", target_path.display()))?,
        None => DocumentMut::new(),
    };

    if !merge_toml_table(target.as_table_mut(), source.as_table(), false)? {
        return Ok(());
    }

    let mut next = target.to_string();
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    atomic_write_if_changed(&target_path, before, &next, simulate)
}

fn capture_toml(projection: &Projection, base_dir: &Path, simulate: bool) -> Result<()> {
    let source_path = expand_path(&projection.source, base_dir);
    let target_path = expand_path(&projection.target, base_dir);
    let mut source = read_toml_document(&source_path)?;
    let target = read_toml_document(&target_path)?;

    if !capture_toml_table(
        source.as_table_mut(),
        target.as_table(),
        false,
        &mut Vec::new(),
    )? {
        return Ok(());
    }

    let mut next = source.to_string();
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    atomic_write_if_changed(
        &source_path,
        FileSnapshot::read(&source_path)?,
        &next,
        simulate,
    )
}

fn read_toml_document(path: &Path) -> Result<DocumentMut> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    content
        .parse::<DocumentMut>()
        .with_context(|| format!("Failed to parse {}", path.display()))
}

fn merge_toml_table(
    target: &mut dyn TomlTableLike,
    source: &dyn TomlTableLike,
    target_is_inline: bool,
) -> Result<bool> {
    let mut changed = false;

    for (key, source_item) in source.iter() {
        if let Some(source_table) = source_item.as_table_like() {
            if let Some(target_item) = target.get_mut(key) {
                match target_item {
                    TomlItem::Table(target_table) => {
                        changed |= merge_toml_table(target_table, source_table, false)
                            .with_context(|| {
                                format!("Failed to merge TOML table at key '{}'", key)
                            })?;
                    }
                    TomlItem::Value(TomlValue::InlineTable(target_table)) => {
                        changed |= merge_toml_table(target_table, source_table, true)
                            .with_context(|| {
                                format!("Failed to merge TOML table at key '{}'", key)
                            })?;
                    }
                    _ => {
                        return Err(anyhow!(
                            "Target TOML path must be a table at key '{}'",
                            key
                        ));
                    }
                }
            } else {
                let inserted = if target_is_inline {
                    TomlItem::Value(toml_item_into_value(source_item)?)
                } else {
                    prepare_inserted_toml_item(source_item, target.is_empty())
                };
                target.insert(key, inserted);
                changed = true;
            }
        } else if let Some(target_item) = target.get_mut(key) {
            if !toml_items_equal(target_item, source_item) {
                replace_toml_item(target_item, source_item, target_is_inline)?;
                changed = true;
            }
        } else {
            let inserted = if target_is_inline {
                TomlItem::Value(toml_item_into_value(source_item)?)
            } else {
                source_item.clone()
            };
            target.insert(key, inserted);
            changed = true;
        }
    }

    Ok(changed)
}

fn capture_toml_table(
    source: &mut dyn TomlTableLike,
    target: &dyn TomlTableLike,
    source_is_inline: bool,
    path: &mut Vec<String>,
) -> Result<bool> {
    let mut changed = false;

    for (key, source_item) in source.iter_mut() {
        let key_str = key.get().to_string();
        path.push(key_str.clone());

        let result = (|| -> Result<bool> {
            let target_item = target
                .get(&key_str)
                .ok_or_else(|| anyhow!("Projection path '{}' not found in target", path.join(".")))?;

            match source_item {
                TomlItem::Table(source_table) if !source_table.is_empty() => {
                    let target_table = target_item.as_table_like().ok_or_else(|| {
                        anyhow!("Target TOML path '{}' must be a table", path.join("."))
                    })?;
                    capture_toml_table(source_table, target_table, false, path)
                }
                TomlItem::Value(TomlValue::InlineTable(source_table))
                    if !source_table.is_empty() =>
                {
                    let target_table = target_item.as_table_like().ok_or_else(|| {
                        anyhow!("Target TOML path '{}' must be a table", path.join("."))
                    })?;
                    capture_toml_table(source_table, target_table, true, path)
                }
                _ => {
                    if toml_items_equal(source_item, target_item) {
                        Ok(false)
                    } else {
                        replace_toml_item(source_item, target_item, source_is_inline)?;
                        Ok(true)
                    }
                }
            }
        })();

        path.pop();
        changed |= result?;
    }

    Ok(changed)
}

fn replace_toml_item(
    dest: &mut TomlItem,
    src: &TomlItem,
    dest_is_inline: bool,
) -> Result<()> {
    match (dest, src) {
        (TomlItem::Value(dest_val), TomlItem::Value(src_val)) => {
            let mut replacement = src_val.clone();
            *replacement.decor_mut() = dest_val.decor().clone();
            *dest_val = replacement;
            Ok(())
        }
        (dest_item, src_item) if dest_is_inline => {
            let mut replacement = toml_item_into_value(src_item)?;
            if let TomlItem::Value(dest_val) = dest_item {
                *replacement.decor_mut() = dest_val.decor().clone();
            }
            *dest_item = TomlItem::Value(replacement);
            Ok(())
        }
        (dest_item, src_item) => {
            *dest_item = src_item.clone();
            Ok(())
        }
    }
}

fn toml_item_into_value(item: &TomlItem) -> Result<TomlValue> {
    match item {
        TomlItem::Value(val) => Ok(val.clone()),
        TomlItem::Table(table) => {
            let mut inline = toml_edit::InlineTable::new();
            for (k, v) in table.iter() {
                inline.insert(k, toml_item_into_value(v)?);
            }
            Ok(TomlValue::InlineTable(inline))
        }
        TomlItem::ArrayOfTables(aot) => {
            let mut array = toml_edit::Array::new();
            for table in aot.iter() {
                let mut inline = toml_edit::InlineTable::new();
                for (k, v) in table.iter() {
                    inline.insert(k, toml_item_into_value(v)?);
                }
                array.push(TomlValue::InlineTable(inline));
            }
            Ok(TomlValue::Array(array))
        }
        TomlItem::None => Err(anyhow!("Unexpected empty TOML item")),
    }
}

fn toml_items_equal(a: &TomlItem, b: &TomlItem) -> bool {
    match (a, b) {
        (TomlItem::Value(va), TomlItem::Value(vb)) => toml_values_equal(va, vb),
        (TomlItem::Table(ta), TomlItem::Table(tb)) => toml_table_likes_equal(ta, tb),
        (TomlItem::Table(ta), TomlItem::Value(TomlValue::InlineTable(tb)))
        | (TomlItem::Value(TomlValue::InlineTable(tb)), TomlItem::Table(ta)) => {
            toml_table_likes_equal(ta, tb)
        }
        (TomlItem::ArrayOfTables(aa), TomlItem::ArrayOfTables(ab)) => {
            aa.len() == ab.len()
                && aa
                    .iter()
                    .zip(ab.iter())
                    .all(|(ta, tb)| toml_table_likes_equal(ta, tb))
        }
        (TomlItem::None, TomlItem::None) => true,
        _ => false,
    }
}

fn toml_values_equal(a: &TomlValue, b: &TomlValue) -> bool {
    match (a, b) {
        (TomlValue::String(sa), TomlValue::String(sb)) => sa.value() == sb.value(),
        (TomlValue::Integer(ia), TomlValue::Integer(ib)) => ia.value() == ib.value(),
        (TomlValue::Float(fa), TomlValue::Float(fb)) => {
            fa.value().to_bits() == fb.value().to_bits()
        }
        (TomlValue::Boolean(ba), TomlValue::Boolean(bb)) => ba.value() == bb.value(),
        (TomlValue::Datetime(da), TomlValue::Datetime(db)) => da.value() == db.value(),
        (TomlValue::Array(aa), TomlValue::Array(ab)) => {
            aa.len() == ab.len()
                && aa
                    .iter()
                    .zip(ab.iter())
                    .all(|(xa, xb)| toml_values_equal(xa, xb))
        }
        (TomlValue::InlineTable(ta), TomlValue::InlineTable(tb)) => toml_table_likes_equal(ta, tb),
        _ => false,
    }
}

fn toml_table_likes_equal(a: &dyn TomlTableLike, b: &dyn TomlTableLike) -> bool {
    a.len() == b.len()
        && a.iter()
            .all(|(k, va)| b.get(k).is_some_and(|vb| toml_items_equal(va, vb)))
}

fn prepare_inserted_toml_item(item: &TomlItem, target_is_empty: bool) -> TomlItem {
    let mut cloned = item.clone();
    if !target_is_empty {
        ensure_table_leading_blank_line(&mut cloned);
    }
    cloned
}

fn ensure_table_leading_blank_line(item: &mut TomlItem) {
    if let TomlItem::Table(table) = item {
        if table.is_implicit() {
            if let Some((_, first_child)) = table.iter_mut().next() {
                ensure_table_leading_blank_line(first_child);
            }
            return;
        }
        let prefix = table
            .decor()
            .prefix()
            .and_then(|raw| raw.as_str())
            .unwrap_or("")
            .to_owned();
        if !prefix.starts_with('\n') {
            table.decor_mut().set_prefix(format!("\n{}", prefix));
        }
    }
}
