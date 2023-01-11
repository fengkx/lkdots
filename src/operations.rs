use crate::{
    path_util::{pathbuf_to_str, relative_path},
    symlink_util::create_symlink,
};
use anyhow::{anyhow, Context, Result};
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
    if from.ends_with(".enc") {
        return Ok(());
    }
    let parent_dir = Path::new(to.as_ref())
        .parent()
        .context("Not parent dir")?;
    let to_dir = 
        parent_dir
        .to_str()
        .context("Fail to get str path")?;
    
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
            result.push(Op::Mkdirp(parent_path.to_str().unwrap().into()));
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

pub fn excute(ops: &[Op]) -> Result<()> {
    let mut conflicts = vec![];
    for op in ops {
        if let Op::Conflict(p) = op {
            conflicts.push(p);
        }
    }

    if !conflicts.is_empty() {
        let err_log = conflicts
            .iter()
            .map(|&p| format!("{} is existed and conlict to your configuration", p))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(anyhow!(err_log));
    }

    for op in ops {
        match op {
            Op::Existed(p) => {
                info!("existed: {}", p);
            }
            Op::Conflict(p) => {
                info!("conflict: {}", p);
                return Err(anyhow!(
                    "{} is existed and conlict to your configuration",
                    p
                ));
            }
            Op::Mkdirp(p) => {
                create_dir_all(p)?;
                info!("mkdirp: {}", p);
            }
            Op::Symlink(from, to, relative) => {
                info!("symbol link: {} -> {} [{}]", from, to, relative);
                create_symlink(from, to, relative)?;
            }
        }
    }
    Ok(())
}
