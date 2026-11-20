use anyhow::{Context, Result};
use atomicwrites::{AllowOverwrite, AtomicFile, Error as AtomicWriteError};
use pathdiff::diff_paths;
use std::borrow::Cow;
use std::fs::{self, File, Permissions};
use std::io::{self, Error, ErrorKind};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileModePolicy {
    /// Enforce owner-only read/write (`0o600` on Unix), even when overwriting looser files.
    Restricted0600,
    /// Preserve existing file permissions when overwriting; keep secure temp default when creating.
    PreserveExisting,
}

/// Ensure `path` has a non-empty parent directory so sibling temp files can be created in place.
fn ensure_non_empty_parent(path: &Path) -> Cow<'_, Path> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => Cow::Borrowed(path),
        _ => Cow::Owned(Path::new(".").join(path)),
    }
}

#[cfg(unix)]
fn set_restricted_permissions(file: &File) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_restricted_permissions(_file: &File) -> io::Result<()> {
    Ok(())
}

/// Atomically write to `path` via a sibling temporary file, applying `mode_policy` and `sync_all`
/// before renaming into place. If `write_fn` fails, the temporary file is removed and `path` is untouched.
pub fn atomic_write<F>(path: &Path, mode_policy: FileModePolicy, write_fn: F) -> io::Result<()>
where
    F: FnOnce(&mut File) -> io::Result<()>,
{
    let existing_perms = match mode_policy {
        FileModePolicy::PreserveExisting => fs::metadata(path).ok().map(|m| m.permissions()),
        FileModePolicy::Restricted0600 => None,
    };

    let target = ensure_non_empty_parent(path);
    AtomicFile::new(target.as_ref(), AllowOverwrite)
        .write(|file| {
            if mode_policy == FileModePolicy::Restricted0600 {
                set_restricted_permissions(file)?;
            }
            write_fn(file)?;
            if let Some(perms) = existing_perms.as_ref() {
                file.set_permissions(perms.clone())?;
            }
            file.sync_all()
        })
        .map_err(|err| match err {
            AtomicWriteError::Internal(io_err) | AtomicWriteError::User(io_err) => io_err,
        })
}

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

    #[test]
    fn test_ensure_non_empty_parent_borrows_when_parent_present() {
        let with_parent = Path::new("/tmp/file.txt");
        assert!(matches!(
            ensure_non_empty_parent(with_parent),
            Cow::Borrowed(p) if p == with_parent
        ));

        let bare = Path::new("file.txt");
        assert_eq!(
            ensure_non_empty_parent(bare).as_ref(),
            Path::new("./file.txt")
        );
    }

    #[test]
    fn test_atomic_write_rolls_back_on_error() {
        use std::io::Write;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path().join("config.txt");
        fs::write(&target, "original").unwrap();

        let err = atomic_write(&target, FileModePolicy::PreserveExisting, |f| {
            f.write_all(b"partial-uncommitted-data")?;
            Err(io::Error::other("simulated write failure"))
        })
        .unwrap_err();

        assert_eq!(err.kind(), ErrorKind::Other);
        assert_eq!(fs::read_to_string(&target).unwrap(), "original");
        assert_eq!(fs::read_dir(temp_dir.path()).unwrap().count(), 1);
    }
}
