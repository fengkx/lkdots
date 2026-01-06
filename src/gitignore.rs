use crate::config::Config;
use crate::path_util::{pathbuf_to_str, relative_path};
use anyhow::{Context, Result};
use atomicwrites::{AllowOverwrite, AtomicFile};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, Write};
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

    let gitignore_path_ref = gitignore_path.as_ref();
    let gitignore_path_obj = Path::new(gitignore_path_ref);

    // Read existing content (if file exists)
    let mut lines: Vec<String> = Vec::new();
    let mut existing_entries = HashMap::new();
    let mut in_lkdots_section = false;
    let mut lkdots_start_idx = None;
    let mut lkdots_end_idx = None;

    if gitignore_path_obj.exists() {
        let f = File::open(gitignore_path_ref)?;
        let reader = std::io::BufReader::new(f);

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

    // Atomic write: use atomicwrites crate for safe atomic file operations
    let af = AtomicFile::new(gitignore_path_ref, AllowOverwrite);
    af.write(|f| {
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

        Ok::<(), std::io::Error>(())
    })
    .with_context(|| {
        format!(
            "Failed to atomically write gitignore file: {:?}",
            gitignore_path_ref
        )
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Entry, Platform};
    use std::borrow::Cow;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_config(temp_dir: &TempDir, encrypt: bool) -> Config<'static> {
        let test_file = temp_dir.path().join("test_file.txt");
        fs::write(&test_file, "test content").unwrap();

        Config {
            entries: vec![Entry {
                from: Cow::Owned(test_file.to_str().unwrap().to_string()),
                to: Cow::Owned("~/test_link".to_string()),
                platforms: Cow::Owned(vec![Platform::Linux]),
                encrypt,
            }],
            gitignore: temp_dir
                .path()
                .join(".gitignore")
                .to_str()
                .unwrap()
                .to_string(),
        }
    }

    #[test]
    fn test_write_gitignore_simulate() {
        let temp_dir = TempDir::new().unwrap();
        let config = create_test_config(&temp_dir, true);
        // Should not error in simulate mode
        write_gitignore(&config, true).unwrap();
    }

    #[test]
    fn test_write_gitignore_no_encrypt() {
        let temp_dir = TempDir::new().unwrap();
        let config = create_test_config(&temp_dir, false);
        // Should not create gitignore entries if encrypt is false
        write_gitignore(&config, false).unwrap();
    }

    #[test]
    fn test_write_gitignore_with_encrypt() {
        let temp_dir = TempDir::new().unwrap();
        let config = create_test_config(&temp_dir, true);
        write_gitignore(&config, false).unwrap();

        // Check if gitignore file was created
        let gitignore_path = temp_dir.path().join(".gitignore");
        if gitignore_path.exists() {
            let content = fs::read_to_string(&gitignore_path).unwrap();
            assert!(content.contains(GITIGNORE_START_MARKER));
            assert!(content.contains(GITIGNORE_END_MARKER));
        }
    }

    #[test]
    fn test_write_gitignore_empty_entries() {
        let temp_dir = TempDir::new().unwrap();
        let config = Config {
            entries: vec![],
            gitignore: temp_dir
                .path()
                .join(".gitignore")
                .to_str()
                .unwrap()
                .to_string(),
        };
        // Should handle empty entries gracefully
        write_gitignore(&config, false).unwrap();
    }

    #[test]
    fn test_write_gitignore_update_existing_section() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test_file.txt");
        fs::write(&test_file, "test content").unwrap();

        let gitignore_path = temp_dir.path().join(".gitignore");
        
        // Create existing gitignore with lkdots section
        let existing_content = format!(
            "# existing entries\n*.log\n\n{}\nold_entry/*\n!old_entry/*.enc\n{}\n",
            GITIGNORE_START_MARKER, GITIGNORE_END_MARKER
        );
        fs::write(&gitignore_path, &existing_content).unwrap();

        let config = Config {
            entries: vec![Entry {
                from: Cow::Owned(test_file.to_str().unwrap().to_string()),
                to: Cow::Owned("~/test_link".to_string()),
                platforms: Cow::Owned(vec![Platform::Linux]),
                encrypt: true,
            }],
            gitignore: gitignore_path.to_str().unwrap().to_string(),
        };

        write_gitignore(&config, false).unwrap();

        let content = fs::read_to_string(&gitignore_path).unwrap();
        // Should preserve existing entries outside lkdots section
        assert!(content.contains("*.log"));
        // Should have lkdots markers
        assert!(content.contains(GITIGNORE_START_MARKER));
        assert!(content.contains(GITIGNORE_END_MARKER));
    }

    #[test]
    fn test_write_gitignore_simulate_with_existing_section() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test_file.txt");
        fs::write(&test_file, "test content").unwrap();

        let gitignore_path = temp_dir.path().join(".gitignore");
        
        // Create existing gitignore with lkdots section
        let existing_content = format!(
            "{}\nold_entry/*\n{}\n",
            GITIGNORE_START_MARKER, GITIGNORE_END_MARKER
        );
        fs::write(&gitignore_path, &existing_content).unwrap();

        let config = Config {
            entries: vec![Entry {
                from: Cow::Owned(test_file.to_str().unwrap().to_string()),
                to: Cow::Owned("~/test_link".to_string()),
                platforms: Cow::Owned(vec![Platform::Linux]),
                encrypt: true,
            }],
            gitignore: gitignore_path.to_str().unwrap().to_string(),
        };

        // Should not error in simulate mode with existing section
        write_gitignore(&config, true).unwrap();
    }

    #[test]
    fn test_write_gitignore_multiple_encrypt_entries() {
        let temp_dir = TempDir::new().unwrap();
        
        let test_file1 = temp_dir.path().join("test1.txt");
        fs::write(&test_file1, "content1").unwrap();
        
        let test_file2 = temp_dir.path().join("test2.txt");
        fs::write(&test_file2, "content2").unwrap();

        let gitignore_path = temp_dir.path().join(".gitignore");

        let config = Config {
            entries: vec![
                Entry {
                    from: Cow::Owned(test_file1.to_str().unwrap().to_string()),
                    to: Cow::Owned("~/link1".to_string()),
                    platforms: Cow::Owned(vec![Platform::Linux]),
                    encrypt: true,
                },
                Entry {
                    from: Cow::Owned(test_file2.to_str().unwrap().to_string()),
                    to: Cow::Owned("~/link2".to_string()),
                    platforms: Cow::Owned(vec![Platform::Linux]),
                    encrypt: true,
                },
            ],
            gitignore: gitignore_path.to_str().unwrap().to_string(),
        };

        write_gitignore(&config, false).unwrap();

        let content = fs::read_to_string(&gitignore_path).unwrap();
        assert!(content.contains(GITIGNORE_START_MARKER));
        assert!(content.contains(GITIGNORE_END_MARKER));
        // Should have entries for both files
        assert!(content.contains("/*"));
        assert!(content.contains("!"));
    }

    #[test]
    fn test_write_gitignore_preserves_other_content() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test_file.txt");
        fs::write(&test_file, "test content").unwrap();

        let gitignore_path = temp_dir.path().join(".gitignore");
        
        // Create existing gitignore with various content
        let existing_content = "# My project ignores\n*.log\nnode_modules/\n.env\n";
        fs::write(&gitignore_path, existing_content).unwrap();

        let config = Config {
            entries: vec![Entry {
                from: Cow::Owned(test_file.to_str().unwrap().to_string()),
                to: Cow::Owned("~/test_link".to_string()),
                platforms: Cow::Owned(vec![Platform::Linux]),
                encrypt: true,
            }],
            gitignore: gitignore_path.to_str().unwrap().to_string(),
        };

        write_gitignore(&config, false).unwrap();

        let content = fs::read_to_string(&gitignore_path).unwrap();
        // Should preserve all original entries
        assert!(content.contains("*.log"));
        assert!(content.contains("node_modules/"));
        assert!(content.contains(".env"));
        assert!(content.contains("# My project ignores"));
    }
}
