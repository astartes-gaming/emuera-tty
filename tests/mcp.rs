//! `--mcp`: a game served over the Model Context Protocol (JSON-RPC, one
//! message a line on stdin/stdout), driven the way an agent would.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn game(tag: &str, erb: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("emuera-tty-mcp-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("CSV")).unwrap();
    std::fs::create_dir_all(dir.join("ERB")).unwrap();
    std::fs::write(dir.join("CSV/GameBase.csv"), "コード,1\n").unwrap();
    std::fs::write(dir.join("ERB/T.ERB"), erb).unwrap();
    dir
}

/// Send the requests in order (each with the next id), return the answers'
/// raw JSON lines, by id.
fn session(dir: &std::path::Path, calls: &[&str]) -> Vec<String> {
    let mut p = Command::new(env!("CARGO_BIN_EXE_emuera-tty"))
        .arg(dir)
        .arg("--mcp")
        .env("EMUERA_SEED", "1")
        .env("EMUERA_WARMUP", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = p.stdin.take().unwrap();
    let mut out = BufReader::new(p.stdout.take().unwrap());
    let mut answers = Vec::new();
    let init = r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#;
    writeln!(stdin, "{}", init).unwrap();
    writeln!(stdin, r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#).unwrap();
    let mut line = String::new();
    out.read_line(&mut line).unwrap();
    answers.push(line.clone());
    for (i, c) in calls.iter().enumerate() {
        writeln!(stdin, r#"{{"jsonrpc":"2.0","id":{},{}}}"#, i + 1, c).unwrap();
        stdin.flush().unwrap();
        line.clear();
        out.read_line(&mut line).unwrap();
        answers.push(line.clone());
    }
    drop(stdin);
    let _ = p.wait();
    let _ = std::fs::remove_dir_all(dir);
    answers
}

fn call(name: &str, args: &str) -> String {
    format!(r#""method":"tools/call","params":{{"name":"{}","arguments":{}}}"#, name, args)
}

#[test]
fn tools_drive_the_game() {
    let erb = "@SYSTEM_TITLE\nPRINTL title\nPRINTBUTTON \"[Go]\", 7\nPRINTL\nPRINTBUTTON \"Sort\", \"SORT\"\nPRINTL\nINPUT\nFLAG:1 = RESULT\n\
        PRINTFORML got {RESULT}\nWAIT\nPRINTL after wait\nINPUTMOUSEKEY\nPRINTFORML mouse {RESULT:0} %RESULTS%\nINPUTMOUSEKEY\nPRINTFORML key {RESULT:0} {RESULT:1}\nINPUT\nQUIT\n\
        @TRIPLE(X)\n#FUNCTION\nRETURNF X * 3\n";
    let a = session(
        &game("tools", erb),
        &[
            r#""method":"tools/list""#,
            &call("buttons", "{}"),
            &call("eval", r#"{"expression":"TRIPLE(FLAG:1 + 2)"}"#),
            &call("run", r#"{"statement":"FLAG:1 = 10"}"#),
            &call("eval", r#"{"expression":"TRIPLE(FLAG:1)"}"#),
            &call("run", r#"{"statement":"WAIT"}"#),
            &call("input", r#"{"value":"7"}"#),
            &call("state", "{}"),
            &call("input", r#"{"value":"SORT"}"#),
            &call("input", r#"{"value":"key:Enter"}"#),
            &call("input", r#"{"value":"1"}"#),
        ],
    );
    assert!(a[0].contains(r#""serverInfo":{"name":"emuera-tty""#), "{}", a[0]);
    for t in ["screen", "buttons", "input", "eval", "run", "state"] {
        assert!(a[1].contains(&format!(r#""name":"{}""#, t)), "{} missing: {}", t, a[1]);
    }
    assert!(a[2].contains(r#"7 → [Go]\n\"SORT\" → Sort"#), "{}", a[2]);
    assert!(a[3].contains(r#""text":"6""#), "{}", a[3]);
    assert!(a[5].contains(r#""text":"30""#), "{}", a[5]);
    assert!(a[6].contains(r#""isError":true"#) && a[6].contains("WAIT"), "{}", a[6]);
    // the input, the WAIT passed by itself, up to the mouse/key input
    assert!(a[7].contains(r#"got 7\nafter wait"#) && a[7].contains("1 WAIT passed") && a[7].contains("INPUTMOUSEKEY"), "{}", a[7]);
    // (Windows' own separator there, as Emuera shows it: a backslash, escaped in the JSON)
    let file = if cfg!(windows) { r"ファイル名:ERB\\T.ERB" } else { "ファイル名:ERB/T.ERB" };
    assert!(a[8].contains(file) && a[8].contains("SYSTEM_TITLE"), "{}", a[8]);
    // (a string button: its value in RESULTS, as Emuera sets it)
    assert!(a[9].contains("mouse 1 SORT"), "{}", a[9]);
    assert!(a[10].contains("key 3 13"), "{}", a[10]);
    assert!(a[11].contains("the game has ended"), "{}", a[11]);
}

#[test]
fn stop_at_wait_and_bad_requests() {
    let a = session(
        &game("wait", "@SYSTEM_TITLE\nPRINTL one\nWAIT\nPRINTL two\nWAIT\nPRINTL three\nINPUT\n"),
        &[
            r#""method":"nope""#,
            &call("input", r#"{"value":"","stop_at_wait":true}"#),
            &call("input", r#"{"value":""}"#),
            &call("eval", r#"{"expression":"NO_SUCH_THING"}"#),
            &call("state", "{}"),
            &call("screen", "{}"),
            &call("input", r#"{"value":"5"}"#),
        ],
    );
    assert!(a[1].contains(r#""code":-32601"#), "{}", a[1]);
    // (the game started at its first WAIT, which the first input answers)
    // stopping at the next WAIT, then running on to the INPUT
    assert!(a[2].contains("two") && a[2].contains("(WAIT)") && !a[2].contains("three"), "{}", a[2]);
    assert!(a[3].contains("three") && a[3].contains("(INPUT)") && !a[3].contains("WAIT passed"), "{}", a[3]);
    // an error is flagged; state names the system phase
    assert!(a[4].contains(r#""isError":true"#), "{}", a[4]);
    assert!(a[5].contains("--- game phase: TITLE"), "{}", a[5]);
    // what `screen` showed isn't repeated by the next input's answer
    assert!(a[7].contains("--- waiting for") && !a[7].contains("three"), "{}", a[7]);
}
