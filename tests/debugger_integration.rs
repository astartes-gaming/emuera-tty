//! Integration test for the debugger
use std::fs;
use std::path::PathBuf;

fn run_debugger_test(game_dir: &std::path::Path, input: &[u8], breakpoints: &str) -> String {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_emuera-tty"));
    cmd.arg(game_dir)
        .arg("--headless")
        .arg("--echo")
        .env("EMUERA_VM_DEBUG", "1")
        .env("EMUERA_BREAKPOINTS", breakpoints)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().expect("Failed to spawn emuera-tty");

    // Write input
    if let Some(stdin) = child.stdin.as_mut() {
        use std::io::Write;
        stdin.write_all(input).expect("Failed to write to stdin");
    }

    let output = child.wait_with_output().expect("Failed to wait for child");
    String::from_utf8_lossy(&output.stdout).to_string() + &String::from_utf8_lossy(&output.stderr)
}

fn setup_test_game(name: &str, erb_content: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("emuera-tty-debug-test-{}-{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("CSV")).unwrap();
    fs::create_dir_all(dir.join("ERB")).unwrap();
    fs::write(dir.join("ERB/TEST.ERB"), erb_content).unwrap();
    // Copy sample CSV files
    let sample_csv = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sample/CSV");
    if sample_csv.exists() {
        for entry in fs::read_dir(sample_csv).unwrap() {
            let entry = entry.unwrap();
            let fname = entry.file_name();
            fs::copy(entry.path(), dir.join("CSV").join(fname)).unwrap();
        }
    }
    dir
}

#[test]
fn debugger_breakpoint_function() {
    // Simple game that calls a function we can breakpoint on
    let erb = r#"
@SYSTEM_TITLE
CALL TEST_FUNC
QUIT

@TEST_FUNC
PRINTL "In test func"
RETURN
"#;
    let dir = setup_test_game("bp_func", erb);
    let output = run_debugger_test(&dir, b"", "TEST_FUNC");

    // Should hit breakpoint at TEST_FUNC
    assert!(output.contains("Debugger stopped"), "Expected debugger stop, got: {}", output);
    assert!(output.contains("TEST_FUNC"), "Expected TEST_FUNC in output: {}", output);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn debugger_breakpoint_file_line() {
    let erb = r#"
@SYSTEM_TITLE
CALL TEST_FUNC
QUIT

@TEST_FUNC
PRINTL "Line 1"
PRINTL "Line 2"
PRINTL "Line 3"
RETURN
"#;
    let dir = setup_test_game("bp_file", erb);
    // Find the line number of "PRINTL "Line 2""
    let erb_path = dir.join("ERB/TEST.ERB");
    let content = fs::read_to_string(&erb_path).unwrap();
    let line_num = content.lines().position(|l| l.contains("Line 2")).unwrap() + 1; // 1-indexed
    let bp_spec = format!("ERB/TEST.ERB:{}", line_num);

    let output = run_debugger_test(&dir, b"", &bp_spec);

    assert!(output.contains("Debugger stopped"), "Expected debugger stop, got: {}", output);
    // the location as the program names the file (`\` on Windows)
    let at = format!("ERB{}TEST.ERB:{}", std::path::MAIN_SEPARATOR, line_num);
    assert!(output.contains(&at), "Expected breakpoint location in output: {}", output);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn debugger_step_over() {
    let erb = r#"
@SYSTEM_TITLE
CALL OUTER
QUIT

@OUTER
CALL INNER
PRINTL "after inner"
RETURN

@INNER
PRINTL "in inner"
RETURN
"#;
    let dir = setup_test_game("step_over", erb);
    let output = run_debugger_test(&dir, b"", "OUTER");

    // Should stop at OUTER
    assert!(output.contains("Debugger stopped"), "Expected debugger stop: {}", output);
    // Note: can't easily test step over without interactive input

    let _ = fs::remove_dir_all(&dir);
}
