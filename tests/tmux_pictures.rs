//! Pictures inside tmux, end to end: a private tmux server whose client
//! runs on a pseudo-terminal that answers as Ghostty does (no sixel in its
//! attributes), with none of Ghostty's variables in tmux's environment (a
//! server started at login). tmux doesn't hand the terminal's answer to the
//! kitty query back to the pane, so emuera-tty has to learn the terminal
//! from tmux itself; it once took tmux's sixel, which Ghostty can't draw.
//! Skipped where tmux isn't installed.
#![cfg(unix)]
mod common;
use common::{sample_copy, temp_dir};
use std::io::{Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn tmux_in_ghostty_gets_kitty_pictures() {
    if Command::new("tmux").arg("-V").output().map_or(true, |o| !o.status.success()) {
        eprintln!("tmux not installed: skipped");
        return;
    }
    let dir = sample_copy("tmux-pictures");
    std::fs::write(dir.join("ERB/TMUXPIC.ERB"), "@SYSTEM_TITLE\nPRINT_IMG \"ICON\"\nPRINTL\nWAIT\nQUIT\n").unwrap();
    let tmp = temp_dir("tmux-pictures-run");
    let conf = tmp.join("tmux.conf");
    std::fs::write(&conf, "set -g allow-passthrough on\nset -g default-terminal tmux-256color\nset -g status off\n").unwrap();
    let err = tmp.join("stderr");
    let sock = format!("emuera-tty-test-{}", std::process::id());

    // the terminal: a pseudo-terminal of 100x30 cells, 10x19 px each
    let (mut master, slave) = unsafe {
        let (mut m, mut s) = (0, 0);
        let ws = libc::winsize { ws_row: 30, ws_col: 100, ws_xpixel: 1000, ws_ypixel: 570 };
        assert_eq!(libc::openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null(), &ws), 0);
        (std::fs::File::from_raw_fd(m), s)
    };
    let tty = |fd: i32| unsafe { Stdio::from_raw_fd(libc::dup(fd)) };
    let shell = format!("{} {} 2>{}; sleep 1", env!("CARGO_BIN_EXE_emuera-tty"), dir.display(), err.display());
    let mut client = Command::new("tmux");
    client
        .args(["-L", &sock, "-f"])
        .arg(&conf)
        .args(["new-session", "-x", "100", "-y", "30", &shell])
        .env("TERM", "xterm-ghostty")
        .env("EMUERA_WARMUP", "0")
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
    // Ghostty does, until the game has drawn its picture
    let mut seen = Vec::new();
    let mut buf = [0u8; 65536];
    let start = Instant::now();
    unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) };
    while start.elapsed() < Duration::from_secs(20) && !contains(&seen, b"\x1b_Ga=t") {
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
    let _ = master.write_all(b"\r");
    std::thread::sleep(Duration::from_millis(1500));
    let _ = Command::new("tmux").args(["-L", &sock, "kill-server"]).stderr(Stdio::null()).status();
    let _ = child.kill();
    let _ = child.wait();
    let note = std::fs::read_to_string(&err).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(note.contains("graphics backend: kitty-tmux"), "{}", note);
    assert!(contains(&seen, b"\x1b_Ga=t"), "no kitty picture reached the terminal\n{}", note);
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}
