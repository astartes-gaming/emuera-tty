//! `--completions bash|zsh|fish`: shell completion scripts for emuera-tty's
//! options, their values and game folders. `--install` puts them where
//! bash-completion and fish find them (`share/` beside the program, as the
//! manual page); zsh needs that folder in `fpath`.

/// An option and what follows it.
#[derive(Clone, Copy)]
enum Arg {
    None,
    /// one of these words
    Words(&'static [&'static str]),
    File,
    /// something to type (a URL, a size): nothing to offer
    Text,
}

/// Every option `main` takes, with its argument.
const OPTIONS: &[(&str, Arg)] = &[
    ("--choose", Arg::None),
    ("--clone", Arg::Text),
    ("--gfx", Arg::Words(&["auto", "kitty", "iterm", "sixel", "fb", "none"])),
    ("--headless", Arg::None),
    ("--echo", Arg::None),
    ("--mcp", Arg::None),
    ("--check", Arg::None),
    ("--strict", Arg::None),
    ("--game-debug", Arg::None),
    ("--vm-debug", Arg::None),
    ("--launch-flags", Arg::File),
    ("--colors", Arg::Words(&["truecolor", "256", "16"])),
    ("--ambiguous", Arg::Words(&["wide", "narrow", "single", "osc66"])),
    ("--translate-log", Arg::File),
    ("--translate-load", Arg::File),
    ("--translate-storage", Arg::File),
    ("--translation-filter", Arg::File),
    ("--libretranslate", Arg::Text),
    ("--saves", Arg::None),
    ("--backup-saves", Arg::None),
    ("--retro", Arg::None),
    ("--term", Arg::Text),
    ("--serial", Arg::File),
    ("--baud", Arg::Words(&["300", "1200", "2400", "4800", "9600", "19200", "38400", "57600", "115200"])),
    ("--serial-format", Arg::Words(&["8N1", "7E1", "7O1", "8E1", "8O1"])),
    ("--flow", Arg::Words(&["xon", "rts", "none"])),
    ("--size", Arg::Text),
    ("--install", Arg::None),
    ("--upgrade", Arg::None),
    ("--uninstall", Arg::None),
    ("--reset-data", Arg::None),
    ("--manual", Arg::None),
    ("--completions", Arg::Words(&["bash", "zsh", "fish"])),
    ("--help", Arg::None),
];

/// What an option does, for zsh's and fish's descriptions.
fn about(opt: &str) -> &'static str {
    match opt {
        "--choose" => "pick a game folder",
        "--clone" => "clone a game's git repository",
        "--gfx" => "image backend",
        "--headless" => "plain stdin/stdout mode",
        "--echo" => "echo inputs in headless mode",
        "--mcp" => "serve the game to AI agents (MCP)",
        "--check" => "parse the game and report errors",
        "--strict" => "unknown commands are errors",
        "--game-debug" => "debug mode ([IF_DEBUG], ;#; lines)",
        "--vm-debug" => "interactive debugger",
        "--launch-flags" => "read options from a file",
        "--colors" => "colour depth",
        "--ambiguous" => "width of box drawing and symbols",
        "--translate-log" => "append new translations to a file",
        "--translate-load" => "read translations from a log",
        "--translate-storage" => "translation log to read and append",
        "--translation-filter" => "text never translated",
        "--libretranslate" => "LibreTranslate server HOST:PORT",
        "--saves" => "list the game's saves",
        "--backup-saves" => "back up the game's saves",
        "--retro" => "ASCII through terminfo (old terminals)",
        "--term" => "terminfo name of the terminal",
        "--serial" => "play on a serial terminal",
        "--baud" => "serial line speed",
        "--serial-format" => "data bits, parity, stop bits",
        "--flow" => "serial flow control",
        "--size" => "screen size COLSxROWS",
        "--install" => "install to a folder on PATH",
        "--upgrade" => "build and install the latest version",
        "--uninstall" => "remove the installed copy",
        "--reset-data" => "forget settings and the game list",
        "--manual" => "show the manual",
        "--completions" => "print a shell completion script",
        _ => "show the options",
    }
}

/// The completion script for `shell` (bash, zsh or fish).
pub fn script(shell: &str) -> Option<String> {
    match shell {
        "bash" => Some(bash()),
        "zsh" => Some(zsh()),
        "fish" => Some(fish()),
        _ => None,
    }
}

fn bash() -> String {
    let all: Vec<&str> = OPTIONS.iter().map(|(o, _)| *o).collect();
    let mut cases = String::new();
    for (o, a) in OPTIONS {
        let body = match a {
            Arg::None => continue,
            Arg::Words(w) => format!("COMPREPLY=($(compgen -W \"{}\" -- \"$cur\"))", w.join(" ")),
            Arg::File => "COMPREPLY=($(compgen -f -- \"$cur\"))".to_string(),
            Arg::Text => "COMPREPLY=()".to_string(),
        };
        cases += &format!("        {}) {}; return ;;\n", o, body);
    }
    format!(
        "# bash completion for emuera-tty (emuera-tty --completions bash)\n\
_emuera_tty() {{\n\
    local cur=\"${{COMP_WORDS[COMP_CWORD]}}\" prev=\"${{COMP_WORDS[COMP_CWORD-1]}}\"\n\
    case \"$prev\" in\n\
{cases}    esac\n\
    case \"$cur\" in\n\
        --retro=*) COMPREPLY=($(compgen -W \"--retro=line\" -- \"$cur\")); return ;;\n\
        -*) COMPREPLY=($(compgen -W \"{opts}\" -- \"$cur\")); return ;;\n\
    esac\n\
    # a game: a folder or an archive of one\n\
    compopt -o filenames 2>/dev/null\n\
    COMPREPLY=($(compgen -d -- \"$cur\") $(compgen -f -X '!*.@(zip|7z|rar|tar|tgz|gz|xz|bz2|zst)' -- \"$cur\"))\n\
}}\n\
shopt -s extglob 2>/dev/null\n\
complete -F _emuera_tty emuera-tty\n",
        cases = cases,
        opts = all.join(" ")
    )
}

fn zsh() -> String {
    let mut specs = String::new();
    for (o, a) in OPTIONS {
        let what = about(o).replace('[', "\\[").replace(']', "\\]").replace('\'', "'\\''");
        let action = match a {
            Arg::None => String::new(),
            Arg::Words(w) => format!(":value:({})", w.join(" ")),
            Arg::File => ":file:_files".into(),
            Arg::Text => ":value: ".into(),
        };
        specs += &format!("    '{}[{}]{}' \\\n", o, what, action);
    }
    format!(
        "#compdef emuera-tty\n\
# zsh completion for emuera-tty (emuera-tty --completions zsh)\n\
_arguments -s \\\n\
{specs}    '--retro=line[line-by-line output]' \\\n\
    '*:game folder or archive:_files -g \"*(-/) *.(zip|7z|rar|tar|tgz|gz|xz|bz2|zst)\"'\n",
        specs = specs
    )
}

fn fish() -> String {
    let mut s = String::from("# fish completion for emuera-tty (emuera-tty --completions fish)\n");
    for (o, a) in OPTIONS {
        let name = o.trim_start_matches("--");
        let what = about(o).replace('\'', "\\'");
        let extra = match a {
            Arg::None => String::new(),
            Arg::Words(w) => format!(" -x -a '{}'", w.join(" ")),
            Arg::File => " -r -F".into(),
            Arg::Text => " -x".into(),
        };
        s += &format!("complete -c emuera-tty -l {} -d '{}'{}\n", name, what, extra);
    }
    s += "complete -c emuera-tty -a '(__fish_complete_directories)'\n";
    s
}

/// Where `--install` puts the scripts for a program in `bin` (none on
/// Windows): bash-completion and fish look in `share/` beside every PATH
/// folder; zsh only where `fpath` says.
pub fn install_paths(bin: &std::path::Path) -> Vec<(&'static str, std::path::PathBuf)> {
    if cfg!(windows) {
        return Vec::new();
    }
    let Some(share) = bin.parent().map(|p| p.join("share")) else { return Vec::new() };
    vec![
        ("bash", share.join("bash-completion/completions/emuera-tty")),
        ("zsh", share.join("zsh/site-functions/_emuera-tty")),
        ("fish", share.join("fish/vendor_completions.d/emuera-tty.fish")),
    ]
}

#[cfg(test)]
mod tests {
    /// Every option in `--help` is completed, and nothing that isn't one.
    #[test]
    fn options_match_the_usage() {
        let main = include_str!("main.rs");
        let usage = &main[main.find("usage: emuera-tty").unwrap()..];
        let usage = &usage[..usage.find("\")").unwrap()];
        let mut in_usage: Vec<String> = Vec::new();
        for w in usage.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')) {
            if w.starts_with("--") && w.len() > 2 && !in_usage.iter().any(|x| x == w) {
                in_usage.push(w.to_string());
            }
        }
        for o in &in_usage {
            assert!(super::OPTIONS.iter().any(|(x, _)| x == o), "{} is in --help but not completed", o);
        }
        for (o, _) in super::OPTIONS {
            assert!(in_usage.iter().any(|x| x == o) || *o == "--help", "{} is completed but not in --help", o);
        }
    }

    /// The scripts parse in their shells, where those are installed.
    #[test]
    // (Windows' bash.exe is WSL's, which fails without a Linux installed: these are Unix shells' scripts)
    #[cfg_attr(windows, ignore)]
    fn scripts_parse() {
        for (shell, flag) in [("bash", "-n"), ("zsh", "-n"), ("fish", "--no-execute")] {
            let Ok(mut c) = std::process::Command::new(shell)
                .arg(flag)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
            else {
                continue;
            };
            use std::io::Write;
            c.stdin.take().unwrap().write_all(super::script(shell).unwrap().as_bytes()).unwrap();
            let out = c.wait_with_output().unwrap();
            assert!(out.status.success(), "{}: {}", shell, String::from_utf8_lossy(&out.stderr));
        }
    }
}
