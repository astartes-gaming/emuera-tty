//! Sound for PLAYSOUND / PLAYBGM: files from the game's `sound/` folder,
//! played by a player program the system has (mpv, ffplay, mplayer,
//! afplay on macOS; pw-play, paplay or aplay for WAV; PowerShell for WAV
//! on Windows). No audio library is linked, so every build and CPU keeps
//! working, and a system without a player just stays silent.
//!
//! As Emuera-EE: ten effect channels (a new effect takes a free one, else
//! the first) and one looping BGM. A volume change applies to the sounds
//! started after it (players can't be turned down while they play).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Mpv,
    Ffplay,
    Mplayer,
    Afplay,
    PwPlay,
    Paplay,
    Aplay,
    PowerShell,
}

const PLAYERS: &[(Kind, &str)] = &[
    (Kind::Mpv, "mpv"),
    (Kind::Ffplay, "ffplay"),
    (Kind::Mplayer, "mplayer"),
    (Kind::Afplay, "afplay"),
    (Kind::PwPlay, "pw-play"),
    (Kind::Paplay, "paplay"),
    (Kind::Aplay, "aplay"),
    (Kind::PowerShell, "powershell"),
];

/// A player program found on the system.
#[derive(Clone, Debug)]
pub struct Player {
    kind: Kind,
    path: PathBuf,
}

impl Player {
    /// The first player found in PATH (or the one named, if it is there).
    pub fn find(named: Option<&str>) -> Option<Player> {
        let path = std::env::var_os("PATH")?;
        let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
        let exe = if cfg!(windows) { ".exe" } else { "" };
        PLAYERS
            .iter()
            .filter(|(_, n)| named.is_none_or(|w| w == *n))
            .find_map(|&(kind, name)| dirs.iter().map(|d| d.join(format!("{}{}", name, exe))).find(|p| p.is_file()).map(|path| Player { kind, path }))
    }

    /// Can it play this file? (the simple players read WAV and little else)
    fn plays(&self, f: &Path) -> bool {
        let ext = f.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        match self.kind {
            Kind::Mpv | Kind::Ffplay | Kind::Mplayer | Kind::Afplay => true,
            Kind::PwPlay | Kind::Paplay => matches!(ext.as_str(), "wav" | "flac" | "ogg" | "oga"),
            Kind::Aplay | Kind::PowerShell => ext == "wav",
        }
    }

    /// Does it loop a file by itself?
    fn loops(&self) -> bool {
        matches!(self.kind, Kind::Mpv | Kind::Ffplay | Kind::Mplayer)
    }

    /// The command playing `f` once (or forever, for the players that
    /// loop) at `vol` percent.
    fn command(&self, f: &Path, vol: u8, forever: bool) -> Command {
        let mut c = Command::new(&self.path);
        let v = vol.min(100);
        match self.kind {
            Kind::Mpv => {
                c.args(["--no-video", "--really-quiet", "--no-terminal", "--no-config"]).arg(format!("--volume={}", v));
                if forever {
                    c.arg("--loop-file=inf");
                }
                c.arg("--");
            }
            Kind::Ffplay => {
                c.args(["-nodisp", "-autoexit", "-loglevel", "quiet", "-volume"]).arg(v.to_string());
                if forever {
                    c.args(["-loop", "0"]);
                }
            }
            Kind::Mplayer => {
                c.args(["-really-quiet", "-noconsolecontrols", "-nolirc", "-softvol", "-volume"]).arg(v.to_string());
                if forever {
                    c.args(["-loop", "0"]);
                }
            }
            Kind::Afplay => {
                c.arg("-v").arg(format!("{:.2}", v as f32 / 100.0));
            }
            Kind::PwPlay => {
                c.arg("--volume").arg(format!("{:.2}", v as f32 / 100.0));
            }
            Kind::Paplay => {
                c.arg(format!("--volume={}", v as u32 * 65536 / 100));
            }
            Kind::Aplay => {
                c.arg("-q");
            }
            Kind::PowerShell => {
                let p = f.to_string_lossy().replace('\'', "''");
                c.args(["-NoProfile", "-NonInteractive", "-Command"]).arg(format!("(New-Object Media.SoundPlayer '{}').PlaySync()", p));
                return quiet(c);
            }
        }
        c.arg(f);
        quiet(c)
    }
}

/// No terminal I/O for the player, and on Linux it ends with us.
fn quiet(mut c: Command) -> Command {
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(any(target_os = "linux", target_os = "android"))]
    unsafe {
        use std::os::unix::process::CommandExt;
        c.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    c
}

/// One sound playing: a thread that runs the player (again, for repeats
/// and for looping with players that don't loop).
struct Voice {
    stop: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
}

impl Voice {
    fn start(player: &Player, f: &Path, vol: u8, times: Option<u32>) -> Voice {
        let (stop, done, child) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)), Arc::new(Mutex::new(None)));
        let v = Voice { stop: stop.clone(), done: done.clone(), child: child.clone() };
        let (player, f) = (player.clone(), f.to_path_buf());
        let spawned = std::thread::Builder::new().name("sound".into()).spawn(move || {
            // None: forever (the player loops by itself where it can)
            let native = times.is_none() && player.loops();
            let mut left = if native { 1 } else { times.unwrap_or(u32::MAX) };
            while left > 0 && !stop.load(Ordering::Relaxed) {
                let began = std::time::Instant::now();
                let Ok(c) = player.command(&f, vol, native).spawn() else { break };
                *child.lock().unwrap() = Some(c);
                if stop.load(Ordering::Relaxed) {
                    // stopped meanwhile: `stop` may have missed it
                    if let Some(c) = child.lock().unwrap().as_mut() {
                        let _ = c.kill();
                    }
                }
                let ok = loop {
                    // poll so that `stop` can take the child to kill it
                    let r = child.lock().unwrap().as_mut().map(|c| c.try_wait());
                    match r {
                        Some(Ok(Some(st))) => break st.success(),
                        Some(Ok(None)) => std::thread::sleep(std::time::Duration::from_millis(50)),
                        _ => break false,
                    }
                };
                *child.lock().unwrap() = None;
                // a player that fails at once won't do better a second time,
                // and a file over in no time isn't worth looping
                if !ok || (times.is_none() && began.elapsed() < std::time::Duration::from_millis(200)) {
                    break;
                }
                left -= 1;
            }
            done.store(true, Ordering::Relaxed);
        });
        if spawned.is_err() {
            v.done.store(true, Ordering::Relaxed);
        }
        v
    }

    fn playing(&self) -> bool {
        !self.done.load(Ordering::Relaxed)
    }

    fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(c) = self.child.lock().unwrap().as_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// The effect channels and the BGM.
pub struct Sound {
    player: Player,
    se: Vec<Option<Voice>>,
    bgm: Option<Voice>,
    /// the BGM asked for (kept while sound is off, to resume it)
    bgm_file: Option<PathBuf>,
    on: bool,
    pub se_volume: u8,
    pub bgm_volume: u8,
}

impl Sound {
    pub fn new(player: Player) -> Sound {
        Sound { player, se: (0..10).map(|_| None).collect(), bgm: None, bgm_file: None, on: true, se_volume: 100, bgm_volume: 100 }
    }

    /// The Sound setting changed: off stops everything; on brings the BGM
    /// back.
    pub fn set_on(&mut self, on: bool) {
        if on == self.on {
            return;
        }
        self.on = on;
        if on {
            if let Some(f) = self.bgm_file.clone() {
                self.play_bgm(&f);
            }
        } else {
            let f = self.bgm_file.take();
            self.stop_se();
            self.stop_bgm();
            self.bgm_file = f;
        }
    }

    /// PLAYSOUND: play `f` `times` times on a free channel.
    pub fn play_se(&mut self, f: &Path, times: u32) {
        if !self.on || self.se_volume == 0 || !self.player.plays(f) {
            return;
        }
        let k = self.se.iter().position(|v| v.as_ref().is_none_or(|v| !v.playing())).unwrap_or(0);
        if let Some(v) = self.se[k].take() {
            v.stop();
        }
        self.se[k] = Some(Voice::start(&self.player, f, self.se_volume, Some(times.max(1))));
    }

    /// STOPSOUND
    pub fn stop_se(&mut self) {
        for v in self.se.iter_mut().filter_map(|v| v.take()) {
            v.stop();
        }
    }

    /// PLAYBGM: `f`, looped until STOPBGM or the next PLAYBGM.
    pub fn play_bgm(&mut self, f: &Path) {
        self.stop_bgm();
        self.bgm_file = Some(f.to_path_buf());
        if !self.on || self.bgm_volume == 0 || !self.player.plays(f) {
            return;
        }
        self.bgm = Some(Voice::start(&self.player, f, self.bgm_volume, None));
    }

    pub fn stop_bgm(&mut self) {
        self.bgm_file = None;
        if let Some(v) = self.bgm.take() {
            v.stop();
        }
    }
}

impl Drop for Sound {
    fn drop(&mut self) {
        self.stop_se();
        self.stop_bgm();
    }
}

/// Is sound wanted with the Sound setting at `v`? (auto: not over SSH, where
/// it would play on the other machine; `EMUERA_SOUND` wins)
pub fn wanted(v: &str) -> bool {
    let v = std::env::var("EMUERA_SOUND").unwrap_or_else(|_| v.to_string());
    let remote = std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_TTY").is_some();
    v == "on" || (v == "auto" && !remote)
}

/// `name` (`se/ok.wav`, `se\ok.wav`) in the game's `sound/` folder; the
/// case of each part needn't match (games are made on Windows).
pub fn find_file(game_dir: &Path, name: &str) -> Option<PathBuf> {
    let mut p = crate::loader::find_file_ci(game_dir, "sound")?;
    for part in name.split(['/', '\\']).filter(|s| !s.is_empty() && *s != ".") {
        if part == ".." {
            return None;
        }
        let exact = p.join(part);
        p = if exact.exists() { exact } else { crate::loader::find_file_ci(&p, part)? };
    }
    p.is_file().then_some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_files_whatever_their_case() {
        let d = std::env::temp_dir().join(format!("emuera-tty-sound-{}", std::process::id()));
        std::fs::create_dir_all(d.join("Sound/SE")).unwrap();
        std::fs::write(d.join("Sound/SE/Ok.wav"), b"RIFF").unwrap();
        assert_eq!(find_file(&d, "se\\ok.WAV"), Some(d.join("Sound/SE/Ok.wav")));
        assert_eq!(find_file(&d, "se/../../x"), None);
        assert_eq!(find_file(&d, "se/none.wav"), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn players_take_what_they_can_play() {
        let p = Player { kind: Kind::Paplay, path: "paplay".into() };
        assert!(p.plays(Path::new("a.WAV")) && !p.plays(Path::new("a.m4a")));
        let m = Player { kind: Kind::Mpv, path: "mpv".into() };
        let args: Vec<String> = m.command(Path::new("x.m4a"), 40, true).get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert!(args.contains(&"--volume=40".to_string()) && args.contains(&"--loop-file=inf".to_string()) && args.last().unwrap() == "x.m4a");
    }
}
