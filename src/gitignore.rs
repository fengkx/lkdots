use crate::config::Config;
use crate::path_util::{pathbuf_to_str, relative_path};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufRead, Seek, Write};
use std::path::Path;

const GITIGNORE_START_MARKER: &str = "# lkdots start";
const GITIGNORE_END_MARKER: &str = "# lkdots end";

/// Write gitignore entries for encrypted files
/// Uses comment markers to manage auto-generated entries
pub fn write_gitignore(cfg: &Config, simulate: bool) -> Result<()> {
    let gitignore_path = shellexpand::tilde(&cfg.gitignore);
    let dir = pathbuf_to_str(
        Path::new(gitignore_path.as_ref())
            .parent()
            .context("Fail to get git repository root")?,
    )?;

    let mut f = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(gitignore_path.as_ref())?;

    // Read existing content
    let reader = std::io::BufReader::new(&f);
    let mut lines: Vec<String> = Vec::new();
    let mut existing_entries = HashMap::new();
    let mut in_lkdots_section = false;
    let mut lkdots_start_idx = None;
    let mut lkdots_end_idx = None;

    for (idx, line_result) in reader.lines().enumerate() {
        let line = line_result?;

        if line.trim() == GITIGNORE_START_MARKER {
            in_lkdots_section = true;
            lkdots_start_idx = Some(idx);
            continue; // Skip the marker line, we'll regenerate it
        }

        if line.trim() == GITIGNORE_END_MARKER {
            in_lkdots_section = false;
            lkdots_end_idx = Some(idx);
            continue; // Skip the marker line, we'll regenerate it
        }

        if !in_lkdots_section {
            lines.push(line.clone());
            existing_entries.insert(line, true);
        }
    }

    // Generate new entries
    let mut new_entries = Vec::new();
    for e in cfg.entries.iter().filter(|&e| e.encrypt) {
        let relative = relative_path(shellexpand::tilde(e.from.as_ref()).as_ref(), dir)
            .context("Failed to calculate relative path for gitignore entry")?;
        let p = relative.to_string_lossy();
        let patterns = vec![format!("{}/*", p), format!("!{}/*.enc", p)];
        for s in patterns {
            if !existing_entries.contains_key(&s) {
                new_entries.push(s);
            }
        }
    }

    if new_entries.is_empty() && lkdots_start_idx.is_none() {
        // No new entries and no existing section, nothing to do
        return Ok(());
    }

    if simulate {
        if lkdots_start_idx.is_some() {
            println!("{}", GITIGNORE_START_MARKER);
        }
        for entry in &new_entries {
            println!("{}", entry);
        }
        if lkdots_end_idx.is_some() {
            println!("{}", GITIGNORE_END_MARKER);
        }
        return Ok(());
    }

    // Reconstruct file: existing content + lkdots section
    f.set_len(0)?; // Truncate file
    f.seek(std::io::SeekFrom::Start(0))?;

    // Write existing content (outside lkdots section)
    for line in &lines {
        writeln!(f, "{}", line)?;
    }

    // Write lkdots section if there are entries
    if !new_entries.is_empty() || lkdots_start_idx.is_some() {
        writeln!(f, "{}", GITIGNORE_START_MARKER)?;
        for entry in &new_entries {
            writeln!(f, "{}", entry)?;
        }
        writeln!(f, "{}", GITIGNORE_END_MARKER)?;
    }

    Ok(())
}
