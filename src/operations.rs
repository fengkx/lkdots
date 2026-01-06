use crate::{
    output::{print_error, print_info, print_success},
    path_util::{pathbuf_to_str, relative_path},
    symlink_util::create_symlink,
};
use anyhow::{Context, Result, anyhow};
use log::info;
use std::{
    borrow::Cow,
    fs::{create_dir_all, read_dir},
    io::ErrorKind,
    path::Path,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Mkdirp(String),
    Symlink(String, String, String),

    Existed(String),
    Conflict(String),
}

impl std::fmt::Display for Op {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Op::Mkdirp(p) => write!(f, "create dir {}", p),
            Op::Symlink(from, to, relative) => write!(
                f,
                "create symbol link {} -> {} relative: {}",
                from, to, relative
            ),
            Op::Existed(p) => write!(f, "{} is existed", p),
            Op::Conflict(p) => write!(f, "{} is existed and conflicted", p),
        }
    }
}

pub fn link_file_or_dir(from: Cow<str>, to: Cow<str>, result: &mut Vec<Op>) -> Result<()> {
    let metadata = Path::new(to.as_ref()).symlink_metadata();
    if let Ok(metadata) = metadata {
        // file existed
        if metadata.is_symlink() {
            let sym_target = std::fs::canonicalize(to.as_ref());
            if let Err(err) = sym_target.as_ref() {
                if err.kind() == ErrorKind::NotFound {
                    result.push(Op::Conflict(to.to_string()));
                    return Ok(());
                }
            }
            let sym_target = sym_target?;
            let sym_target = sym_target.to_str().context("Fail to get str path")?;
            let abs_from = std::fs::canonicalize(from.as_ref())?;
            let abs_from = abs_from.to_str().context("Fail to get str path")?;
            if sym_target != abs_from {
                result.push(Op::Conflict(to.to_string()));
            } else {
                result.push(Op::Existed(to.to_string()));
            }
        } else if metadata.is_dir() {
            link_dir(from, to, result)?;
        } else {
            result.push(Op::Conflict(to.to_string()));
        }
    } else {
        let from_path = Path::new(from.as_ref());
        if from_path.symlink_metadata()?.is_dir() {
            link_dir(from, to, result)?;
        } else {
            link_file(from, to, result)?;
        };
    }
    Ok(())
}

fn link_file(from: Cow<str>, to: Cow<str>, res: &mut Vec<Op>) -> Result<()> {
    // Skip encrypted files, don't create symlinks
    if from.ends_with(".enc") {
        return Ok(());
    }
    let parent_dir = Path::new(to.as_ref()).parent().context("Not parent dir")?;
    let to_dir = parent_dir.to_str().context("Fail to get str path")?;

    if !parent_dir.exists() {
        res.push(Op::Mkdirp(to_dir.into()));
    }
    let relative = relative_path(from.as_ref(), to_dir)?;

    res.push(Op::Symlink(
        from.to_string(),
        to.to_string(),
        relative.to_string_lossy().to_string(),
    ));
    Ok(())
}

fn link_dir(from: Cow<str>, to: Cow<str>, result: &mut Vec<Op>) -> Result<()> {
    let relative = {
        let to_path = Path::new(to.as_ref());
        let to_dir = to_path
            .parent()
            .context("Not parent dir")?
            .to_str()
            .context("Fail to get str path")?;

        relative_path(from.as_ref(), to_dir)?
    };
    let to_path = Path::new(to.as_ref());
    if !to_path.exists() {
        // create_dir_all(to_path.parent().unwrap_or(Path::new("/")))?;
        let parent_path = to_path.parent().unwrap_or_else(|| Path::new("/"));
        if !parent_path.exists() {
            let parent_str = parent_path
                .to_str()
                .context("Parent path contains invalid UTF-8 characters")?;
            result.push(Op::Mkdirp(parent_str.into()));
        }
        result.push(Op::Symlink(
            from.into(),
            to.into(),
            relative.to_str().context("Fail to get str path")?.into(),
        ));
    } else {
        // directory existed, link files in directory
        for f in read_dir(from.as_ref())? {
            let f = f?;
            let from_path = f.path().to_path_buf();
            let from_str = pathbuf_to_str(&from_path)?;

            let fname = f.file_name();
            let fname = fname.to_str().context("Fail to get str path")?;

            let to_path = Path::new(to.as_ref()).join(fname);

            let to_str = to_path.to_str().context("Fail to get str path")?;

            // println!("{:?} {:?}", from_path, to_str);
            link_file_or_dir(Cow::Borrowed(from_str), Cow::Borrowed(to_str), result)?;
        }
    }
    Ok(())
}

pub fn execute(ops: &[Op]) -> Result<()> {
    // 先收集所有冲突
    let conflicts: Vec<&String> = ops
        .iter()
        .filter_map(|op| match op {
            Op::Conflict(p) => Some(p),
            _ => None,
        })
        .collect();

    if !conflicts.is_empty() {
        print_error(&format!("Found {} conflict(s)", conflicts.len()));
        println!("\nConflicting files:");
        for conflict in &conflicts {
            println!("  - {}", conflict);
        }
        let err_log = format!(
            "\nResolution suggestions:\n\
            1. Manually remove or rename the conflicting files\n\
            2. Use --simulate mode to preview operations\n\
            3. Check path settings in configuration file\n\
            4. Backup important files before overwriting"
        );
        return Err(anyhow!(err_log));
    }

    for op in ops {
        match op {
            Op::Conflict(_) => unreachable!("Conflicts should be handled above"),
            Op::Existed(p) => {
                info!("existed: {}", p);
                print_info(&format!("Already exists: {}", p));
            }
            Op::Mkdirp(p) => {
                create_dir_all(p)?;
                info!("mkdirp: {}", p);
                print_success(&format!("Created directory: {}", p));
            }
            Op::Symlink(from, to, relative) => {
                info!("symbol link: {} -> {} [{}]", from, to, relative);
                create_symlink(from, to, relative)?;
                print_success(&format!("Created symlink: {} -> {}", to, from));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_op_display() {
        let op1 = Op::Mkdirp("/test/dir".to_string());
        let op2 = Op::Symlink(
            "/from".to_string(),
            "/to".to_string(),
            "../from".to_string(),
        );
        let op3 = Op::Existed("/existing".to_string());
        let op4 = Op::Conflict("/conflict".to_string());

        let s1 = format!("{}", op1);
        assert!(s1.contains("create dir"));
        assert!(s1.contains("/test/dir"));

        let s2 = format!("{}", op2);
        assert!(s2.contains("create symbol link"));
        assert!(s2.contains("/from"));
        assert!(s2.contains("/to"));

        let s3 = format!("{}", op3);
        assert!(s3.contains("is existed"));

        let s4 = format!("{}", op4);
        assert!(s4.contains("is existed and conflicted"));
    }

    #[test]
    fn test_link_file_or_dir_with_nonexistent_target() {
        let temp_dir = TempDir::new().unwrap();
        let from_file = temp_dir.path().join("test_file.txt");
        fs::write(&from_file, "test content").unwrap();

        let to_file = temp_dir.path().join("link_file.txt");
        let mut ops = Vec::new();
        link_file_or_dir(
            Cow::Borrowed(from_file.to_str().unwrap()),
            Cow::Borrowed(to_file.to_str().unwrap()),
            &mut ops,
        )
        .unwrap();

        assert!(!ops.is_empty());
        assert!(ops.iter().any(|op| matches!(op, Op::Symlink(_, _, _))));
    }

    #[test]
    fn test_link_file_or_dir_skip_encrypted() {
        let temp_dir = TempDir::new().unwrap();
        let from_file = temp_dir.path().join("test_file.enc");
        fs::write(&from_file, "encrypted content").unwrap();

        let to_file = temp_dir.path().join("link_file.enc");
        let mut ops = Vec::new();
        link_file_or_dir(
            Cow::Borrowed(from_file.to_str().unwrap()),
            Cow::Borrowed(to_file.to_str().unwrap()),
            &mut ops,
        )
        .unwrap();

        // Should skip encrypted files
        assert!(ops.is_empty() || !ops.iter().any(|op| matches!(op, Op::Symlink(_, _, _))));
    }

    #[test]
    fn test_execute_with_conflicts() {
        let ops = vec![
            Op::Conflict("/conflict1".to_string()),
            Op::Conflict("/conflict2".to_string()),
        ];
        let result = execute(&ops);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_mkdirp() {
        let temp_dir = TempDir::new().unwrap();
        let new_dir = temp_dir.path().join("new_subdir");
        let ops = vec![Op::Mkdirp(new_dir.to_str().unwrap().to_string())];
        execute(&ops).unwrap();
        assert!(new_dir.exists());
        assert!(new_dir.is_dir());
    }

    #[test]
    fn test_execute_existed() {
        let ops = vec![Op::Existed("/existing/path".to_string())];
        // Should not error, just print info
        execute(&ops).unwrap();
    }

    #[test]
    fn test_execute_symlink() {
        let temp_dir = TempDir::new().unwrap();
        let from_file = temp_dir.path().join("source.txt");
        fs::write(&from_file, "test content").unwrap();

        let to_file = temp_dir.path().join("link.txt");
        // The symlink uses relative path from the link location to the source
        let relative = "source.txt".to_string();

        let ops = vec![Op::Symlink(
            from_file.to_str().unwrap().to_string(),
            to_file.to_str().unwrap().to_string(),
            relative,
        )];
        execute(&ops).unwrap();
        // Check that symlink was created
        assert!(to_file.symlink_metadata().unwrap().is_symlink());
        // The symlink should resolve to the source file
        assert!(to_file.exists());
    }

    #[test]
    fn test_link_file_or_dir_existing_symlink_same_target() {
        let temp_dir = TempDir::new().unwrap();
        let from_file = temp_dir.path().join("source.txt");
        fs::write(&from_file, "test content").unwrap();

        let to_file = temp_dir.path().join("link.txt");
        // Create the symlink first
        #[cfg(unix)]
        std::os::unix::fs::symlink(&from_file, &to_file).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&from_file, &to_file).unwrap();

        let mut ops = Vec::new();
        link_file_or_dir(
            Cow::Borrowed(from_file.to_str().unwrap()),
            Cow::Borrowed(to_file.to_str().unwrap()),
            &mut ops,
        )
        .unwrap();

        // Should detect as Existed since symlink points to correct target
        assert!(ops.iter().any(|op| matches!(op, Op::Existed(_))));
    }

    #[test]
    fn test_link_file_or_dir_existing_symlink_different_target() {
        let temp_dir = TempDir::new().unwrap();
        let from_file = temp_dir.path().join("source.txt");
        fs::write(&from_file, "test content").unwrap();

        let other_file = temp_dir.path().join("other.txt");
        fs::write(&other_file, "other content").unwrap();

        let to_file = temp_dir.path().join("link.txt");
        // Create symlink to other_file (not from_file)
        #[cfg(unix)]
        std::os::unix::fs::symlink(&other_file, &to_file).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&other_file, &to_file).unwrap();

        let mut ops = Vec::new();
        link_file_or_dir(
            Cow::Borrowed(from_file.to_str().unwrap()),
            Cow::Borrowed(to_file.to_str().unwrap()),
            &mut ops,
        )
        .unwrap();

        // Should detect as Conflict since symlink points to different target
        assert!(ops.iter().any(|op| matches!(op, Op::Conflict(_))));
    }

    #[test]
    fn test_link_file_or_dir_broken_symlink() {
        let temp_dir = TempDir::new().unwrap();
        let from_file = temp_dir.path().join("source.txt");
        fs::write(&from_file, "test content").unwrap();

        let nonexistent = temp_dir.path().join("nonexistent.txt");
        let to_file = temp_dir.path().join("broken_link.txt");

        // Create symlink to nonexistent file (broken symlink)
        #[cfg(unix)]
        std::os::unix::fs::symlink(&nonexistent, &to_file).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&nonexistent, &to_file).unwrap();

        let mut ops = Vec::new();
        link_file_or_dir(
            Cow::Borrowed(from_file.to_str().unwrap()),
            Cow::Borrowed(to_file.to_str().unwrap()),
            &mut ops,
        )
        .unwrap();

        // Should detect as Conflict for broken symlink
        assert!(ops.iter().any(|op| matches!(op, Op::Conflict(_))));
    }

    #[test]
    fn test_link_file_or_dir_existing_regular_file_conflict() {
        let temp_dir = TempDir::new().unwrap();
        let from_file = temp_dir.path().join("source.txt");
        fs::write(&from_file, "test content").unwrap();

        let to_file = temp_dir.path().join("existing.txt");
        fs::write(&to_file, "existing content").unwrap();

        let mut ops = Vec::new();
        link_file_or_dir(
            Cow::Borrowed(from_file.to_str().unwrap()),
            Cow::Borrowed(to_file.to_str().unwrap()),
            &mut ops,
        )
        .unwrap();

        // Should detect as Conflict since to_file is a regular file
        assert!(ops.iter().any(|op| matches!(op, Op::Conflict(_))));
    }

    #[test]
    fn test_link_dir_nonexistent_target() {
        let temp_dir = TempDir::new().unwrap();
        let from_dir = temp_dir.path().join("source_dir");
        fs::create_dir(&from_dir).unwrap();
        fs::write(from_dir.join("file.txt"), "content").unwrap();

        let to_dir = temp_dir.path().join("link_dir");

        let mut ops = Vec::new();
        link_file_or_dir(
            Cow::Borrowed(from_dir.to_str().unwrap()),
            Cow::Borrowed(to_dir.to_str().unwrap()),
            &mut ops,
        )
        .unwrap();

        // Should create symlink for directory
        assert!(ops.iter().any(|op| matches!(op, Op::Symlink(_, _, _))));
    }

    #[test]
    fn test_link_dir_existing_target_recursive() {
        let temp_dir = TempDir::new().unwrap();
        let from_dir = temp_dir.path().join("source_dir");
        fs::create_dir(&from_dir).unwrap();
        fs::write(from_dir.join("file1.txt"), "content1").unwrap();
        fs::write(from_dir.join("file2.txt"), "content2").unwrap();

        let to_dir = temp_dir.path().join("target_dir");
        fs::create_dir(&to_dir).unwrap();

        let mut ops = Vec::new();
        link_file_or_dir(
            Cow::Borrowed(from_dir.to_str().unwrap()),
            Cow::Borrowed(to_dir.to_str().unwrap()),
            &mut ops,
        )
        .unwrap();

        // Should create symlinks for files in directory (recursive linking)
        let symlink_count = ops
            .iter()
            .filter(|op| matches!(op, Op::Symlink(_, _, _)))
            .count();
        assert!(symlink_count >= 2);
    }

    #[test]
    fn test_link_file_creates_parent_dir() {
        let temp_dir = TempDir::new().unwrap();
        let from_file = temp_dir.path().join("source.txt");
        fs::write(&from_file, "test content").unwrap();

        let to_file = temp_dir.path().join("subdir/nested/link.txt");

        let mut ops = Vec::new();
        link_file_or_dir(
            Cow::Borrowed(from_file.to_str().unwrap()),
            Cow::Borrowed(to_file.to_str().unwrap()),
            &mut ops,
        )
        .unwrap();

        // Should include Mkdirp for parent directory
        assert!(ops.iter().any(|op| matches!(op, Op::Mkdirp(_))));
        assert!(ops.iter().any(|op| matches!(op, Op::Symlink(_, _, _))));
    }
}
