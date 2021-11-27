use crate::symlink_util::create_symlink;
use std::fs::create_dir_all;

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Mkdirp(String),
    Rimraf(String),
    Symlink(String, String),
    Unlink(String),

    Existed(String),
    Conflict(String),
}
