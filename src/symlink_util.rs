use std::{
    fs::Metadata,
    io::{Error, ErrorKind, Result},
    os::unix::fs::symlink,
    path::Path,
};

pub fn get_symbol_meta_data(p: &str) -> Result<Metadata> {
    let p = Path::new(p);
    p.symlink_metadata()
}

pub fn create_symlink(_src: &str, dst: &str, relative: &str) -> Result<()> {
    // Check if parent directory is writable
    let dst_path = Path::new(dst);
    if let Some(parent) = dst_path.parent() {
        let parent_metadata = parent.metadata();
        if let Ok(meta) = parent_metadata {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = meta.permissions();
                // Check if directory is writable (owner, group, or other write permission)
                if perms.mode() & 0o222 == 0 {
                    return Err(Error::new(
                        ErrorKind::PermissionDenied,
                        format!("{} is not writable", parent.display()),
                    ));
                }
            }
        }
    }

    // Create symlink - std::os::unix::fs::symlink works for both files and directories
    symlink(relative, dst)
}

#[test]
fn test_get_metadata() {
    let metadata = get_symbol_meta_data("/etc/passwd").unwrap();
    assert!(metadata.is_file());
    let metadata = get_symbol_meta_data("./tests/test-data").unwrap();
    assert!(metadata.is_dir());

    assert!(get_symbol_meta_data("/etc/localtime").unwrap().is_symlink());
    assert!(!Path::new("/etc/localtime").metadata().unwrap().is_symlink());
    assert!(Path::new("/etc/localtime").metadata().unwrap().is_file());
}

#[test]
fn test_permission() {
    // Test that we can check permissions using standard library
    let home_metadata = std::fs::metadata(env!("HOME")).unwrap();
    assert!(home_metadata.is_dir());
}
