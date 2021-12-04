use permissions::{is_creatable, is_writable};
use std::{
    fs::Metadata,
    io::{Error, ErrorKind, Result},
    path::Path,
};

pub fn get_symbol_meta_data(p: &str) -> Result<Metadata> {
    let p = Path::new(p);
    p.symlink_metadata()
}

pub fn create_symlink(src: &str, dst: &str, relative: &str) -> Result<()> {
    if !is_creatable(dst)? && !is_writable(dst)? {
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            format!("{} is not writable", dst),
        ));
    }

    let metadata = get_symbol_meta_data(src)?;
    if metadata.is_dir() {
        symlink::symlink_dir(relative, dst)
    } else {
        symlink::symlink_file(relative, dst)
    }
}

#[test]
fn test_get_metadata() {
    let metadata = get_symbol_meta_data("/etc/passwd").unwrap();
    assert!(metadata.is_file());
    let metadata = get_symbol_meta_data("/etc").unwrap();
    assert!(metadata.is_dir());
    assert!(get_symbol_meta_data("/etc/localtime").unwrap().is_symlink());
    assert!(!get_meta_data("/etc/localtime").unwrap().is_symlink());
    assert!(get_meta_data("/etc/localtime").unwrap().is_file());
}

#[test]
fn test_permission() {
    use permissions::is_writable;
    assert!(!is_writable("/etc/passwd").unwrap());
    assert!(is_writable(env!("HOME")).unwrap());
}
