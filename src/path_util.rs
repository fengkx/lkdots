use std::io::{Error, ErrorKind, Result};
use std::path::Path;

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

pub fn get_dir(p: &Path) -> Result<&Path> {
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
