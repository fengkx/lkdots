use crate::operations::{Op, link_file_or_dir};
use anyhow::{Context, Result};
use log::debug;
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, ffi::OsString, path::Path};

pub const PLATFORM: &str = if cfg!(target_os = "linux") {
    "linux"
} else if cfg!(target_os = "windows") {
    "windows"
} else if cfg!(target_os = "macos") {
    "darwin"
} else {
    "linux"
};

// serde

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Linux,
    Darwin,
    Window,
}

impl PartialEq<Platform> for str {
    fn eq(&self, other: &Platform) -> bool {
        match other {
            Platform::Linux => self == "linux",
            Platform::Darwin => self == "darwin",
            Platform::Window => self == "windows",
        }
    }
}

impl PartialEq<str> for Platform {
    fn eq(&self, other: &str) -> bool {
        other == self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFileEntry {
    pub from: String,
    pub to: String,
    pub platforms: Option<Vec<Platform>>,
    pub encrypt: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFileStruct {
    pub entries: Vec<ConfigFileEntry>,
    pub gitignore: String,
}

// END serde

#[derive(Debug, Clone)]
pub struct Entry<'a> {
    pub from: Cow<'a, String>,
    pub to: Cow<'a, String>,
    pub platforms: Cow<'a, Vec<Platform>>,
    pub encrypt: bool,
}

impl Entry<'_> {
    pub fn create_ops(&self, base_dir: &Path) -> Result<Vec<Op>> {
        let from_osstr: OsString = if self.from.starts_with('/') || self.from.starts_with('~') {
            self.from.as_ref().into()
        } else {
            base_dir.join(self.from.as_ref()).into_os_string()
        };
        let from = from_osstr
            .to_str()
            .context("Path contains invalid UTF-8 characters")?;
        let from = shellexpand::tilde(from);
        let to = shellexpand::tilde(self.to.as_ref());
        debug!("from: {}, to: {}", from, to);
        let mut result = Vec::<Op>::new();
        link_file_or_dir(from, to, &mut result)?;
        Ok(result)
    }
    pub fn match_platform(&self) -> bool {
        self.platforms.iter().any(|p| p == PLATFORM)
    }
}

#[derive(Debug, Clone)]
pub struct Config<'a> {
    pub entries: Vec<Entry<'a>>,
    pub gitignore: String,
}

impl From<ConfigFileStruct> for Config<'static> {
    fn from(c: ConfigFileStruct) -> Self {
        Config {
            gitignore: c.gitignore,
            entries: c
                .entries
                .into_iter()
                .map(|e| Entry {
                    from: Cow::Owned(e.from),
                    to: Cow::Owned(e.to),
                    platforms: Cow::Owned(e.platforms.unwrap_or_else(|| {
                        vec![Platform::Linux, Platform::Darwin, Platform::Window]
                    })),
                    encrypt: e.encrypt.unwrap_or(false),
                })
                .collect(),
        }
    }
}

impl Config<'_> {
    /// Validate configuration entries
    /// Checks if source paths exist and if paths are valid
    pub fn validate(&self) -> Result<()> {
        use std::path::Path;

        if self.entries.is_empty() {
            return Err(anyhow::anyhow!(
                "Configuration error: No entries found in config file"
            ));
        }

        for (idx, entry) in self.entries.iter().enumerate() {
            let expanded_from = shellexpand::tilde(entry.from.as_ref());
            let from_path = Path::new(expanded_from.as_ref());

            // Check if source path exists
            if !from_path.exists() {
                return Err(anyhow::anyhow!(
                    "Configuration error in entry #{}: Source path does not exist\n\
                    Path: {}",
                    idx + 1,
                    entry.from
                ));
            }

            // Validate target path is not empty
            if entry.to.is_empty() {
                return Err(anyhow::anyhow!(
                    "Configuration error in entry #{}: Target path is empty",
                    idx + 1
                ));
            }

            // Validate gitignore path if entries require encryption
            if entry.encrypt && !self.gitignore.is_empty() {
                let expanded_gitignore = shellexpand::tilde(&self.gitignore);
                let gitignore_parent = Path::new(expanded_gitignore.as_ref()).parent();

                if gitignore_parent.is_none() {
                    return Err(anyhow::anyhow!(
                        "Configuration error: Invalid gitignore path (no parent directory)\n\
                        Path: {}",
                        self.gitignore
                    ));
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_match_platform() {
        let entry = Entry {
            from: Cow::Owned("test".to_string()),
            to: Cow::Owned("test".to_string()),
            platforms: Cow::Owned(vec![Platform::Linux, Platform::Darwin]),
            encrypt: false,
        };
        // This will match on Linux/Darwin but not Windows
        let matches = entry.match_platform();
        // On macOS, this should match
        #[cfg(target_os = "macos")]
        assert!(matches);
        #[cfg(target_os = "linux")]
        assert!(matches);
        #[cfg(target_os = "windows")]
        assert!(!matches);
    }

    #[test]
    fn test_config_from() {
        let config_file = ConfigFileStruct {
            entries: vec![
                ConfigFileEntry {
                    from: "test1".to_string(),
                    to: "test2".to_string(),
                    platforms: Some(vec![Platform::Linux]),
                    encrypt: Some(true),
                },
                ConfigFileEntry {
                    from: "test3".to_string(),
                    to: "test4".to_string(),
                    platforms: None,
                    encrypt: None,
                },
            ],
            gitignore: ".gitignore".to_string(),
        };

        let config: Config = config_file.into();
        assert_eq!(config.entries.len(), 2);
        assert_eq!(config.gitignore, ".gitignore");
        assert!(config.entries[0].encrypt);
        assert!(!config.entries[1].encrypt);
        // Default platforms should include all
        assert_eq!(config.entries[1].platforms.len(), 3);
    }

    #[test]
    fn test_config_validate_empty_entries() {
        let config = Config {
            entries: vec![],
            gitignore: ".gitignore".to_string(),
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validate_nonexistent_path() {
        let config = Config {
            entries: vec![Entry {
                from: Cow::Owned("/nonexistent/path".to_string()),
                to: Cow::Owned("~/test".to_string()),
                platforms: Cow::Owned(vec![Platform::Linux]),
                encrypt: false,
            }],
            gitignore: ".gitignore".to_string(),
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validate_empty_to() {
        let config = Config {
            entries: vec![Entry {
                from: Cow::Owned("./tests/test-data".to_string()),
                to: Cow::Owned("".to_string()),
                platforms: Cow::Owned(vec![Platform::Linux]),
                encrypt: false,
            }],
            gitignore: ".gitignore".to_string(),
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validate_success() {
        let config = Config {
            entries: vec![Entry {
                from: Cow::Owned("./tests/test-data".to_string()),
                to: Cow::Owned("~/test".to_string()),
                platforms: Cow::Owned(vec![Platform::Linux]),
                encrypt: false,
            }],
            gitignore: ".gitignore".to_string(),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_entry_create_ops() {
        use std::fs;
        use tempfile::TempDir;

        // Create a temporary directory structure for testing
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test_file.txt");
        fs::write(&test_file, "test content").unwrap();

        let entry = Entry {
            from: Cow::Owned(test_file.to_str().unwrap().to_string()),
            to: Cow::Owned(
                temp_dir
                    .path()
                    .join("link.txt")
                    .to_str()
                    .unwrap()
                    .to_string(),
            ),
            platforms: Cow::Owned(vec![Platform::Linux]),
            encrypt: false,
        };
        let base_dir = temp_dir.path();
        let ops = entry.create_ops(base_dir).unwrap();
        assert!(!ops.is_empty());
    }
}
