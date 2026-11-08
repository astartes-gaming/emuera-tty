//! Pictures inside tmux, end to end: a private tmux server whose client
//! runs on a pseudo-terminal that answers as Ghostty does (no sixel in its
//! attributes), with none of Ghostty's variables in tmux's environment (a
//! server started at login). tmux doesn't hand the terminal's answer to the
//! kitty query back to the pane, so emuera-tty has to learn the terminal
//! from tmux itself; it once took tmux's sixel, which Ghostty can't draw.
//! Without `allow-passthrough` no picture can get through, so emuera-tty
//! stops at start and says what to set. Skipped where tmux isn't installed.
#![cfg(unix)]
mod common;
use common::{sample_copy, temp_dir};
use std::io::{Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// What a run in tmux in Ghostty showed: the game's stderr, its exit
/// status (None: still running when tmux was stopped), and every byte
/// that reached the terminal.
struct Run {
    stderr: String,
    status: Option<i32>,
    seen: Vec<u8>,
}

fn tmux_missing() -> bool {
    let missing = Command::new("tmux").arg("-V").output().map_or(true, |o| !o.status.success());
    if missing {
        eprintln!("tmux not installed: skipped");
    }
    missing
}

/// Run the picture game with `args` in a private tmux (with or without
/// `allow-passthrough`) whose client answers as Ghostty, until it has
/// drawn a kitty picture, quit, or said it runs without pictures; then
/// Enter (ending its WAIT) and stop tmux.
fn run_in_ghostty_tmux(tag: &str, passthrough: bool, args: &str) -> Run {
    let dir = sample_copy(tag);
    std::fs::write(dir.join("ERB/TMUXPIC.ERB"), "@SYSTEM_TITLE\nPRINT_IMG \"ICON\"\nPRINTL\nWAIT\nQUIT\n").unwrap();
    let tmp = temp_dir(&format!("{}-run", tag));
    let conf = tmp.join("tmux.conf");
    let pass = if passthrough { "set -g allow-passthrough on\n" } else { "set -g allow-passthrough off\n" };
    std::fs::write(&conf, format!("{}set -g default-terminal tmux-256color\nset -g status off\n", pass)).unwrap();
    let (err, status) = (tmp.join("stderr"), tmp.join("status"));
    let sock = format!("emuera-tty-{}-{}", tag, std::process::id());

    // the terminal: a pseudo-terminal of 100x30 cells, 10x19 px each
    let (mut master, slave) = unsafe {
        let (mut m, mut s) = (0, 0);
        let mut ws = libc::winsize { ws_row: 30, ws_col: 100, ws_xpixel: 1000, ws_ypixel: 570 };
        assert_eq!(libc::openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null_mut(), &mut ws), 0);
        (std::fs::File::from_raw_fd(m), s)
    };
    let tty = |fd: i32| unsafe { Stdio::from_raw_fd(libc::dup(fd)) };
    let shell = format!("{} {} {} 2>{}; echo $? >{}; sleep 1", env!("CARGO_BIN_EXE_emuera-tty"), dir.display(), args, err.display(), status.display());
    let mut client = Command::new("tmux");
    client
        .args(["-L", &sock, "-f"])
        .arg(&conf)
        .args(["new-session", "-x", "100", "-y", "30", &shell])
        .env("TERM", "xterm-ghostty")
        .env("EMUERA_WARMUP", "0")
        .env("EMUERA_TTY_TESTING", "1")
        .env_remove("TMUX")
        .env_remove("TERM_PROGRAM")
        .env_remove("GHOSTTY_RESOURCES_DIR")
        .env_remove("KITTY_WINDOW_ID")
        .stdin(tty(slave))
        .stdout(tty(slave))
        .stderr(tty(slave));
    unsafe {
        client.pre_exec(move || {
            libc::setsid();
            libc::ioctl(slave, libc::TIOCSCTTY as _, 0);
            Ok(())
        });
    }
    let mut child = client.spawn().unwrap();
    unsafe { libc::close(slave) };

    // read what reaches the terminal, answering the attributes query as
    // Ghostty does
    let mut seen = Vec::new();
    let mut buf = [0u8; 65536];
    let start = Instant::now();
    unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) };
    let settled =
        |seen: &[u8]| contains(seen, b"\x1b_Ga=t") || status.exists() || std::fs::read_to_string(&err).is_ok_and(|e| e.contains("graphics backend: none"));
    while start.elapsed() < Duration::from_secs(20) && !settled(&seen) {
        match master.read(&mut buf) {
            Ok(n) if n > 0 => {
                let new = &buf[..n];
                if contains(new, b"\x1b[c") || contains(new, b"\x1b[0c") {
                    let _ = master.write_all(b"\x1b[?62;22c");
                }
                seen.extend_from_slice(new);
            }
            _ => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    // Enter ends the WAIT, and the game quits
    std::thread::sleep(Duration::from_millis(300));
    let _ = master.write_all(b"\r");
    std::thread::sleep(Duration::from_millis(1500));
    let _ = Command::new("tmux").args(["-L", &sock, "kill-server"]).stderr(Stdio::null()).status();
    let _ = child.kill();
    let _ = child.wait();
    let run = Run {
        stderr: std::fs::read_to_string(&err).unwrap_or_default(),
        status: std::fs::read_to_string(&status).ok().and_then(|s| s.trim().parse().ok()),
        seen,
    };
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&tmp);
    run
}

#[test]
fn tmux_in_ghostty_gets_kitty_pictures() {
    if tmux_missing() {
        return;
    }
    let r = run_in_ghostty_tmux("tmux-pictures", true, "");
    assert!(r.stderr.contains("graphics backend: kitty-tmux"), "{}", r.stderr);
    assert!(contains(&r.seen, b"\x1b_Ga=t"), "no kitty picture reached the terminal\n{}", r.stderr);
}

/// Without `allow-passthrough` the game doesn't start: it says what to set
#[test]
fn tmux_without_passthrough_says_what_to_set() {
    if tmux_missing() {
        return;
    }
    let r = run_in_ghostty_tmux("tmux-no-passthrough", false, "");
    assert_eq!(r.status, Some(1), "{}", r.stderr);
    assert!(r.stderr.contains("set -g allow-passthrough all"), "{}", r.stderr);
    assert!(!r.stderr.contains("graphics backend"), "{}", r.stderr);
}

/// ... unless pictures are turned off
#[test]
fn tmux_without_passthrough_plays_without_pictures() {
    if tmux_missing() {
        return;
    }
    let r = run_in_ghostty_tmux("tmux-no-pictures", false, "--gfx none");
    assert!(r.stderr.contains("graphics backend: none"), "{}", r.stderr);
    assert_eq!(r.status, Some(0), "{}", r.stderr);
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}
