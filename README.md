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
    pub platforms: Option<Vec<Platfrom>>,
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
