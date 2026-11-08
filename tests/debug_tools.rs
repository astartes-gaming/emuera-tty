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
