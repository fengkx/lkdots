use anyhow::{Context, Result};
use pathdiff::diff_paths;
use std::io::{self, Error, ErrorKind};
use std::path::{Path, PathBuf};

pub fn find_existed_up(p: &str) -> Option<&Path> {
    let mut p = Path::new(p);
    if p.exists() {
        return Some(p);
    }
    while let Some(parent) = p.parent() {
        if parent.exists() {
            return Some(parent);
        }
        p = parent;
    }
    return None;
}

pub fn get_dir(p: &Path) -> io::Result<&Path> {
    let metadata = p.metadata()?;
    if metadata.is_dir() {
        Ok(p)
    } else {
        match p.parent() {
            Some(p) => Ok(p),
            None => Err(Error::new(ErrorKind::NotFound, "No parent dir")),
        }
    }
}

#[inline]
pub fn relative_path(from: &str, to: &str) -> anyhow::Result<PathBuf> {
    diff_paths(from, to).context(format!(
        "Fail to find relative path from {} to {}",
        from, to
    ))
}

#[inline]
pub fn pathbuf_to_str<'a>(pb: &'a PathBuf) -> Result<&'a str> {
    pb.to_str().context("path is not valid str")
}
