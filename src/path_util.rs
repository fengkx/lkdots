use anyhow::{Context, Result};
use pathdiff::diff_paths;
use std::io::{self, Error, ErrorKind};
use std::path::{Path, PathBuf};

pub fn get_dir(p: &Path) -> io::Result<&Path> {
    let metadata = p.metadata()?;
    if metadata.is_dir() {
        Ok(p)
    } else {
        match p.parent() {
            Some(p) => Ok(p),
            None => Err(Error::new(
                ErrorKind::NotFound,
                format!("No parent dir on {:?}", p),
            )),
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
pub fn pathbuf_to_str(pb: &Path) -> Result<&str> {
    pb.to_str().context("path is not valid str")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_dir_with_file() {
        let path = Path::new("./Cargo.toml");
        let dir = get_dir(path).unwrap();
        assert!(dir.is_dir());
        assert_eq!(dir, Path::new("."));
    }

    #[test]
    fn test_get_dir_with_directory() {
        let path = Path::new("./tests");
        let dir = get_dir(path).unwrap();
        assert_eq!(dir, path);
    }

    #[test]
    fn test_relative_path() {
        let from = "/a/b/c";
        let to = "/a/b";
        let relative = relative_path(from, to).unwrap();
        assert_eq!(relative, PathBuf::from("c"));
    }

    #[test]
    fn test_relative_path_same_dir() {
        let from = "/a/b/c";
        let to = "/a/b/c";
        let relative = relative_path(from, to).unwrap();
        // When paths are the same, diff_paths returns None or empty path
        assert!(relative == PathBuf::from(".") || relative == PathBuf::from(""));
    }

    #[test]
    fn test_pathbuf_to_str() {
        let path = Path::new("./tests/test-data");
        let str_path = pathbuf_to_str(path).unwrap();
        assert_eq!(str_path, "./tests/test-data");
    }
}
