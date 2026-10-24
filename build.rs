//! Puts the git commit this build comes from in `EMUERA_COMMIT` ("unknown"
//! outside a git checkout), for the save log (`savesafe.rs`).

fn main() {
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "--short=10", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=EMUERA_COMMIT={}", commit);
    // again when the checked-out commit changes (a commit, a checkout)
    for f in [".git/HEAD", ".git/logs/HEAD"] {
        if std::path::Path::new(f).exists() {
            println!("cargo:rerun-if-changed={}", f);
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
}
