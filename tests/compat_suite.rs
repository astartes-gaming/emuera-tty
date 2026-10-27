//! Runs the eraBasic compatibility suite (`compat/`, the submodule
//! https://gitgud.io/era-games/erabasic-compat): its expected values are
//! Emuera-EM/EE's, so every check must pass here too.
mod common;
use common::{run, temp_dir};
use std::path::Path;

#[test]
#[ignore = "emuera-tty doesn't pass the suite yet (GAMEBASE_GAMECODE stops it); run with --ignored"]
fn compat_suite_passes() {
    let src = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/compat"));
    // a clone without --recurse-submodules, or a source archive, has an
    // empty compat/: nothing to run
    if !src.join("ERB").is_dir() {
        eprintln!("compat/ is empty: run `git submodule update --init` to test it");
        return;
    }
    // only the game itself: not the emuera.config or log of an Emuera run
    // in the checkout, and the run's own emuera.log goes to the copy
    let dir = temp_dir("compat");
    for sub in ["CSV", "ERB"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
        for e in std::fs::read_dir(src.join(sub)).unwrap() {
            let e = e.unwrap();
            std::fs::copy(e.path(), dir.join(sub).join(e.file_name())).unwrap();
        }
    }
    let text = run(&dir, b"\n");
    let ng: Vec<&str> = text.lines().filter(|l| l.starts_with("NG  ") || l.starts_with("SKIP ")).collect();
    assert!(!text.contains("script error"), "{}", text);
    assert!(ng.is_empty(), "{}", ng.join("\n"));
    let summary = text.lines().find(|l| l.starts_with("== ")).unwrap_or_else(|| panic!("no summary\n{}", text));
    assert!(summary.contains(" 0 failed"), "{}", summary);
    let _ = std::fs::remove_dir_all(&dir);
}
