//! Emuera's debug features: `@` commands typed at the game's input
//! (doSystemCommand) and DEBUGPRINT. The debug panel itself is the TUI's
//! (`src/tty/debugpanel.rs`, `src/vm/debugwin.rs`).
mod common;
use common::{game, run, temp_dir};

fn play(tag: &str, config: &str, input: &[u8], debug: bool) -> String {
    let dir = temp_dir(tag);
    let erb = "@SYSTEM_TITLE\nADDVOIDCHARA\nNAME:0 = Reimu\nFLAG:3 = 1\nINPUT\nPRINTFORML got {RESULT} flag {FLAG:3} name %NAME:0%\nQUIT\n";
    game(&dir, &[("emuera.config", config), ("ERB/T.ERB", erb)]);
    let out = if debug {
        let exe = env!("CARGO_BIN_EXE_emuera-tty");
        let mut c = std::process::Command::new(exe);
        c.arg(&dir).args(["--headless", "--echo", "--game-debug"]).env("EMUERA_TTY_TESTING", "1").env("XDG_DATA_HOME", dir.join("data"));
        c.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        let mut ch = c.spawn().unwrap();
        use std::io::Write;
        let _ = ch.stdin.take().unwrap().write_all(input);
        let o = ch.wait_with_output().unwrap();
        String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
    } else {
        run(&dir, input, None)
    };
    let _ = std::fs::remove_dir_all(&dir);
    out
}

/// デバッグコマンドを使用する on: a statement runs (and MASTER is renamed
/// イカサマ, デバッグコマンドを使用した時にMASTERの名前を変更する being on
/// by default), an expression prints its value, Emuera's restrictions
/// apply; the input is then asked for again.
#[test]
fn at_commands_run_statements_and_expressions() {
    let out = play("at-on", "デバッグコマンドを使用する:YES\n", b"@FLAG:3 = 5\n@FLAG:3 * 2\n@WAIT\n@GOTO X\n7\n", false);
    for want in ["\n10\n", "WAIT命令は使用できません", "フロー制御命令は使用できません", "got 7 flag 5 name イカサマ"] {
        assert!(out.contains(want), "{want:?} missing:\n{out}");
    }
    let keep = play("at-keep-name", "デバッグコマンドを使用する:YES\nデバッグコマンドを使用した時にMASTERの名前を変更する:NO\n", b"@FLAG:3 = 5\n7\n", false);
    assert!(keep.contains("got 7 flag 5 name Reimu"), "{keep}");
}

/// Off (Emuera's default): Emuera's message, nothing runs; @DEBUG needs
/// debug mode; @QUIT quits.
#[test]
fn at_commands_follow_the_settings() {
    let off = play("at-off", "", b"@FLAG:3 = 5\n@DEBUG\n7\n", false);
    for want in ["デバッグコマンドを使用できない設定になっています", "デバッグウインドウは-Debug引数付きで起動したときのみ使えます", "got 7 flag 1 name Reimu"]
    {
        assert!(off.contains(want), "{want:?} missing:\n{off}");
    }
    let quit = play("at-quit", "", b"@QUIT\n7\n", false);
    assert!(!quit.contains("got 7"), "{quit}");
    let debug = play("at-debug", "", b"@DEBUG\n7\n", true);
    assert!(!debug.contains("デバッグウインドウは") && debug.contains("got 7"), "{debug}");
}

/// In debug mode the load warnings come first in the game's console, each
/// followed by the line it is about (Emuera's PrintWarning).
#[test]
fn load_warnings_are_printed_in_debug_mode() {
    let dir = temp_dir("load-warnings-debug");
    game(
        &dir,
        &[
            ("emuera.config", "同名の非イベント関数が複数定義されたとき警告する:YES\n"),
            ("ERB/T.ERB", "@SYSTEM_TITLE\nPRINTL title\nQUIT\n@A\nRETURN\n@A\nRETURN 1\n"),
        ],
    );
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_emuera-tty"))
        .arg(&dir)
        .args(["--headless", "--echo", "--game-debug"])
        .env("EMUERA_TTY_TESTING", "1")
        .env("XDG_DATA_HOME", dir.join("data"))
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    let warn = "警告Lv1:ERB/T.ERB:6行目:関数\"@A\"は既に定義(ERB/T.ERBの4行目)されています\n@A\n";
    assert!(out.find(warn).is_some_and(|w| out.find("title").is_some_and(|t| w < t)), "{out}");
    // not in normal play
    assert!(!run(&dir, b"", None).contains("警告"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// @REBOOT reloads ERB files (like F5), preserving game state.
#[test]
fn at_reboot_reloads_erb() {
    let dir = temp_dir("at-reboot");
    game(
        &dir,
        &[
            ("emuera.config", "デバッグコマンドを使用する:YES\n"),
            ("ERB/T.ERB", "@SYSTEM_TITLE\nADDVOIDCHARA\nNAME:0 = Reimu\nFLAG:3 = 10\nINPUT\nPRINTFORML flag={FLAG:3}\nQUIT\n"),
        ],
    );
    let out = run(&dir, b"@REBOOT\n7\n", None);
    assert!(out.contains("Reloading ERB"));
    assert!(out.contains("flag=10"), "state preserved after reboot: {out}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// @CONFIG reloads config in headless mode.
#[test]
fn at_config_reloads_config() {
    let dir = temp_dir("at-config");
    game(&dir, &[("emuera.config", "デバッグコマンドを使用する:YES\n"), ("ERB/T.ERB", "@SYSTEM_TITLE\nADDVOIDCHARA\nINPUT\nPRINTFORML done\nQUIT\n")]);
    let out = run(&dir, b"@CONFIG\n7\n", None);
    assert!(out.contains("Config reloaded"), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// @OUTPUTLOG writes the console to a file.
#[test]
fn at_outputlog_writes_file() {
    let dir = temp_dir("at-outputlog");
    game(&dir, &[("emuera.config", "デバッグコマンドを使用する:YES\n"), ("ERB/T.ERB", "@SYSTEM_TITLE\nADDVOIDCHARA\nPRINTL hello\n@OUTPUTLOG\nQUIT\n")]);
    let out = run(&dir, b"@OUTPUTLOG\n", None);
    assert!(out.contains("Log written to"), "{out}");
    let log = std::fs::read_to_string(dir.join("emuera.log")).unwrap();
    assert!(log.contains("hello"), "log content: {log}");
    assert!(log.contains("@OUTPUTLOG"), "log content: {log}");
    let _ = std::fs::remove_dir_all(&dir);
}
