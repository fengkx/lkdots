# lkdots

> A cli tool to create symbol link of dotfiles with encryption and more(maybe)

# Usage

```
A cli tool to create symbol link of dotfiles with encryption and more

USAGE:
    lkdots [FLAGS] [OPTIONS] [SUBCOMMAND]

FLAGS:
    -h, --help        Prints help information
        --simulate    simulate fs operations, do not actually make any filesystem changes
    -V, --version     Prints version information

OPTIONS:
    -c <config>        path to config file [default: /home/fengkx/project/lkdots/lkdots.toml]

SUBCOMMANDS:
    decrypt    decrypt files to original position
    encrypt    encrypt files to *.enc file
    help       Prints this message or the help of the given subcommand(s)
```

# Config

[example](https://github.com/fengkx/dotfiles/tree/master/lkdots.toml)

## gitignore

path of the `.gitignore` in git repository

## entries

Array of entries to "link".

```rust
pub struct ConfigFileEntry {
    pub from: String,
    pub to: String,
    pub platforms: Option<Vec<Platform>>,
    pub encrypt: Option<bool>,
}
```

### from

path of dotfile source

### to

link destination of entry

### platforms

array of `"linux", "window", "darwin"`

### encrypt

whether encrypt this entry

### examples

```toml
[[entries]]
from = "~/dotfiles/ssh"
to = "~/.ssh"
encrypt = true
```

`lkdots encrypt` will create encrypted `.enc` file in the same directory (unencrypted files will be added to `.gitignore`)  
`lkdots decrypt` will recover all uncrypted files  
`lkdtos` will link `~/dotfiles/ssh` to `~/.ssh`.

## projections

Array of managed projections for files that should not be linked wholesale.

```toml
[[projections]]
name = "npmrc"
driver = "properties"
source = "~/dotfiles/projections/npmrc.toml"
target = "~/.npmrc"

[[projections]]
name = "codex-global-state"
driver = "json"
source = "~/dotfiles/projections/codex-global-state.json"
target = "~/.codex/.codex-global-state.json"
```

`lkdots apply` writes projection sources into target files. Running `lkdots`
without a subcommand also applies projections after linking entries.

`lkdots capture` extracts the same managed projection fields from target files
back into the source files.

### properties driver

The source is a TOML file with a `[values]` table:

```toml
[values]
registry = "https://registry.npmjs.org/"
"@tencent:registry" = "https://mirrors.tencent.com/npm/"
```

The target is a line-oriented `key=value` file such as `.npmrc`. Managed keys
are replaced or appended once; duplicate managed keys in the target are
normalized. Unmanaged lines, comments, and local secret lines are preserved.
Keys that look like tokens or passwords are rejected.

### json driver

The source is a partial JSON document:

```json
{
  "electron-persisted-atom-state": {
    "git-commit-instructions": "Use concise commit messages."
  }
}
```

Objects are merged recursively. Scalar values and arrays are replaced. Fields
not present in the source partial are left untouched in the target.
