use crate::operations::{link_file_or_dir, Op};
use anyhow::Result;
use log::debug;
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, ffi::OsString, path::Path};

pub const PLATFORM: &str = if cfg!(target_os = "linux") {
    "linux"
} else if cfg!(target_os = "windows") {
    "window"
} else if cfg!(target_os = "macos") {
    "darwin"
} else {
    "linux"
};

// serde

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platfrom {
    Linux,
    Darwin,
    Window,
}

impl PartialEq<Platfrom> for str {
    fn eq(&self, other: &Platfrom) -> bool {
        match other {
            Platfrom::Linux => self == "linux",
            Platfrom::Darwin => self == "darwin",
            Platfrom::Window => self == "window",
        }
    }
}

impl PartialEq<str> for Platfrom {
    fn eq(&self, other: &str) -> bool {
        other == self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFileEntry {
    pub from: String,
    pub to: String,
    pub platforms: Option<Vec<Platfrom>>,
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
    pub platforms: Cow<'a, Vec<Platfrom>>,
    pub encrypt: bool,
}

impl<'a> Entry<'a> {
    pub fn create_ops(&self, base_dir: &Path) -> Result<Vec<Op>> {
        let from_osstr: OsString = if self.from.starts_with('/') || self.from.starts_with('~') {
            self.from.as_ref().into()
        } else {
            base_dir.join(&self.from.as_ref()).into_os_string()
        };
        let from = from_osstr.to_str().unwrap();
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
                        vec![Platfrom::Linux, Platfrom::Darwin, Platfrom::Window]
                    })),
                    encrypt: e.encrypt.unwrap_or(false),
                })
                .collect(),
        }
    }
}
