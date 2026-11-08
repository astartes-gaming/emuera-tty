//! emuera-tty: run Emuera (ERB) games in a terminal.
//!
//! Startup: parse options, read CSVs (`csvdata`), parse every .ERH then
//! .ERB file (`parser`), load image resources, pick a frontend (full-screen
//! `tty` or `--headless`), read the game's config files, and hand over to
//! the VM's system loop on a thread with a large stack (ERB code can
//! recurse deeply, and each ERB call is a Rust call).

// first: its `t!` / `tc!` macros are used by the modules below
#[macro_use]
mod i18n;

mod archive;
mod ast;
mod atomic64;
mod clipboard;
mod config_names;
mod console;
mod csvdata;
mod download;
mod emuconfig;
mod expr;
mod frontend;
mod fxhash;
#[cfg(target_os = "linux")]
mod gamepad;
#[cfg(not(target_os = "linux"))]
#[path = "gamepad_none.rs"]
mod gamepad;
mod gfx;
mod html;
mod json;
mod lint;
mod loader;
mod macros;
#[cfg(target_os = "linux")]
mod mice;
#[cfg(not(target_os = "linux"))]
#[path = "mice_none.rs"]
mod mice;
mod mt;
mod parser;
mod picker;
mod regex;
mod resources;
mod retro;
mod savemgr;
mod savesafe;
mod settings;
mod sound;
mod sys;
mod terminfo;
mod translate;
mod tty;
mod vm;
mod xml;

use std::path::PathBuf;

/// Command-line options.
struct Opts {
    /// None: use the current folder, the remembered game, or the picker
    game: Option<PathBuf>,
    /// --clone URL (or a URL as GAME_DIR): a git repository the launcher clones
    clone: Option<String>,
    /// --choose: open the picker even if a game is remembered
    choose: bool,
    headless: bool,
    gfx: String,
    strict: bool,
    check: bool,
    game_debug: bool,
    vm_debug: bool,
    echo: bool,
    /// --retro / --retro=line: the retro-terminal frontend (otherwise used
    /// only when $TERM isn't ANSI)
    retro: Option<String>,
    /// --term: the terminfo name for the retro / serial terminal
    term: Option<String>,
    /// --serial DEVICE, with --baud / --serial-format / --flow
    serial: Option<String>,
    baud: u32,
    serial_format: String,
    flow: String,
    /// --size COLSxROWS
    size: Option<(usize, usize)>,
    /// --saves / --backup-saves
    saves: Option<&'static str>,
    /// --translate-load FILE (any number), and --translate-storage's file:
    /// translation logs to read translations from
    translate_load: Vec<PathBuf>,
    /// --translate-log FILE, or --translate-storage's: where new
    /// translations are appended
    translate_log: Option<PathBuf>,
    /// --translation-filter FILE (any number): what is never translated
    translation_filter: Vec<PathBuf>,
    /// --launch-flags FILE (repeatable): read launch flags from key=value files
    launch_flags: Vec<PathBuf>,
    /// Colors setting (truecolor|256|16)
    colors: Option<String>,
    /// Ambiguous width setting (wide|narrow|single|osc66)
    ambiguous: Option<String>,
    /// LibreTranslate host:port
    libretranslate: Option<String>,
}

/// Print help; exit 0 when asked for (--help), 2 on bad options.
fn usage_exit(code: i32) -> ! {
    eprintln!(
        "{}",
        t!("usage: emuera-tty [GAME_DIR | GIT_URL] [options]\n\
         \n\
         GAME_DIR may be a game folder or an archive of one (.zip, .tar.gz, .7z…;\n\
         extracted once to ~/.local/share/emuera-tty/games).\n\
         GIT_URL (https://…, git@host:…): the launcher clones the game's repository\n\
         into the library (as its \"Clone a game…\" does) and selects it.\n\
         Without GAME_DIR: the current folder if it is a game, else the game\n\
         chosen last time, else a folder picker opens.\n\
         \n\
         options:\n\
         \x20 --choose                         pick a game folder (even if one is remembered)\n\
         \x20 --clone URL                      clone a game's git repository in the launcher (as GIT_URL)\n\
         \x20 --gfx auto|kitty|iterm|sixel|fb|none image backend (default auto)\n\
         \x20 --headless                       plain stdin/stdout mode (scripts, tests)\n\
         \x20 --echo                           echo inputs in headless mode\n\
         \x20 --check                          parse the game and report errors only\n\
         \x20 --strict                         unknown commands are errors\n\
         \x20 --game-debug                    enable [IF_DEBUG] / ;#; lines\n\
         \x20 --vm-debug                      enable interactive debugger (breakpoints, step, watches)\n\
         \x20 --launch-flags FILE            read launch flags from a key=value file (see below; repeatable)\n\
         \n\
         display (each also has an environment variable, which the option overrides):\n\
         \x20 --colors truecolor|256|16        colour depth, when detection guesses wrong (EMUERA_COLORS)\n\
         \x20 --ambiguous wide|narrow|single|osc66\n\
         \x20                                  how box drawing and symbols get Emuera's two columns (EMUERA_AMBIGUOUS)\n\
         \n\
         translation (the languages and the translator are in the F3 settings):\n\
         \x20 --translate-log FILE             append every new translation to FILE (never reset; no row twice)\n\
         \x20 --translate-load FILE            read translations from a translation log, asked before any\n\
         \x20                                  translator (repeat for several; a later file wins)\n\
         \x20 --translate-storage FILE         both of the above, with the one file\n\
         \x20 --translation-filter FILE        script files and lines of text that are never translated\n\
         \x20                                  (repeat for several; # comments; middle-click still translates)\n\
         \x20 --libretranslate HOST:PORT       the LibreTranslate server (default 127.0.0.1:5000; EMUERA_LIBRETRANSLATE)\n\
         \n\
         saves (also in the F3 overlay: Manage saves):\n\
         \x20 --saves                          list the game's saves with their comments\n\
         \x20 --backup-saves                   copy all of them to a new backup folder\n\
         \n\
         retro and serial terminals:\n\
         \x20 --retro[=line]                   drive the terminal from its terminfo entry, ASCII only\n\
         \x20                                  (automatic when $TERM isn't ANSI: vt52, adm3a, dumb…);\n\
         \x20                                  =line prints line by line, for terminals with no cursor control\n\
         \x20 --term NAME                      terminfo name of the terminal (default $TERM; vt100 on --serial)\n\
         \x20 --serial DEVICE                  play on a terminal on a serial line (/dev/ttyS0, /dev/ttyUSB0…)\n\
         \x20 --baud N                         line speed (default 9600)\n\
         \x20 --serial-format 8N1|7E1|…        data bits, parity, stop bits (default 8N1)\n\
         \x20 --flow xon|rts|none              flow control (default xon: XON/XOFF)\n\
         \x20 --size COLSxROWS                 screen size if the terminal can't report it (default: terminfo)\n\
         \n\
         setup:\n\
         \x20 --install                        copy this program to a folder on your PATH (~/.local/bin…)\n\
         \x20 --upgrade                        fetch, build (git + cargo) and install the latest version\n\
         \x20 --uninstall                      remove the installed copy\n\
         \x20 --reset-data                     forget settings and the game library; saves are kept\n\
         \x20 --manual                         the full manual (also: man emuera-tty once installed)\n\
         \n\
         launch flags file (--launch-flags):\n\
         \x20 a file of key=value lines; # comments; repeat keys allowed for translate-load\n\
         \x20 and translation-filter; booleans are set by presence (e.g. strict=1)\n\
         \x20 excluded: install, uninstall, clone, reset-data, saves, backup-saves,\n\
         \x20 manual, headless, check")
    );
    std::process::exit(code)
}

fn usage() -> ! {
    usage_exit(2)
}

/// Keys that are not allowed in a launch flags file.
const LAUNCH_FLAGS_EXCLUDE: &[&str] = &["install", "uninstall", "clone", "reset-data", "saves", "backup-saves", "manual", "headless", "check"];

/// Parse a launch flags file and apply its contents to `o`.
/// Returns true if the file was read successfully, false on error (with error printed).
fn parse_launch_flags(o: &mut Opts, path: &str) -> bool {
    parse_launch_flags_inner(o, path, &mut std::collections::HashSet::new())
}

fn parse_launch_flags_inner(o: &mut Opts, path: &str, visited: &mut std::collections::HashSet<PathBuf>) -> bool {
    let canonical = match std::fs::canonicalize(path) {
        Ok(c) => c,
        Err(_) => PathBuf::from(path),
    };
    if !visited.insert(canonical.clone()) {
        eprintln!("emuera-tty: --launch-flags: circular include: {}", path);
        return false;
    }
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("emuera-tty: --launch-flags: {}: {}", path, e);
            return false;
        }
    };
    // Resolve relative paths in the launch flags file against the file's directory.
    let base = std::path::Path::new(path).parent().unwrap_or(std::path::Path::new(""));
    let resolve = |p: &str| -> PathBuf {
        let pb = PathBuf::from(p);
        if pb.is_relative() {
            base.join(pb)
        } else {
            pb
        }
    };
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(include_path) = line.strip_prefix('@') {
            let include_path = include_path.trim();
            let resolved = resolve(include_path);
            if !parse_launch_flags_inner(o, &resolved.to_string_lossy(), visited) {
                return false;
            }
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if LAUNCH_FLAGS_EXCLUDE.contains(&key) {
            continue;
        }
        match key {
            "gfx" => o.gfx = value.to_string(),
            "strict" => o.strict = true,
            "game-debug" => o.game_debug = true,
            "vm-debug" => o.vm_debug = true,
            "echo" => o.echo = true,
            "choose" => o.choose = true,
            "retro" => o.retro = Some(if value.is_empty() { "screen".into() } else { value.to_string() }),
            "term" => o.term = Some(value.to_string()),
            "serial" => o.serial = Some(value.to_string()),
            "baud" => o.baud = value.parse().unwrap_or(9600),
            "serial-format" => o.serial_format = value.to_string(),
            "flow" => o.flow = value.to_string(),
            "size" => {
                if let Some((c, r)) = value.split_once(['x', 'X']) {
                    if let (Ok(cols), Ok(rows)) = (c.parse(), r.parse()) {
                        o.size = Some((cols, rows));
                    }
                }
            }
            "translate-log" => o.translate_log = Some(resolve(value)),
            "translate-load" => o.translate_load.push(resolve(value)),
            "translate-storage" => {
                o.translate_load.insert(0, resolve(value));
                if o.translate_log.is_none() {
                    o.translate_log = Some(resolve(value));
                }
            }
            "translation-filter" => o.translation_filter.push(resolve(value)),
            "colors" => o.colors = Some(value.to_string()),
            "ambiguous" => o.ambiguous = Some(value.to_string()),
            "libretranslate" => o.libretranslate = Some(value.to_string()),
            _ => {} // ignore unknown keys
        }
    }
    true
}

/// Parse `std::env::args` (exits with usage on unknown options).
fn parse_args() -> Opts {
    let mut o = Opts {
        game: None,
        clone: None,
        choose: false,
        headless: false,
        gfx: "auto".into(),
        strict: false,
        check: false,
        game_debug: false,
        vm_debug: false,
        echo: false,
        retro: None,
        term: None,
        serial: None,
        baud: 9600,
        serial_format: "8N1".into(),
        flow: "xon".into(),
        size: None,
        saves: None,
        translate_load: Vec::new(),
        translate_log: None,
        translation_filter: Vec::new(),
        launch_flags: Vec::new(),
        colors: None,
        ambiguous: None,
        libretranslate: None,
    };
    let mut storage: Option<PathBuf> = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        // the options with a value, as `--name VALUE` or `--name=VALUE`
        let (name, inline) = match a.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (a.as_str(), None),
        };
        if matches!(
            name,
            "--translate-log" | "--translate-load" | "--translate-storage" | "--translation-filter" | "--colors" | "--ambiguous" | "--libretranslate"
        ) {
            let v = inline.or_else(|| it.next()).filter(|v| !v.is_empty()).unwrap_or_else(|| usage());
            match name {
                "--translate-log" => o.translate_log = Some(PathBuf::from(v)),
                "--translate-load" => o.translate_load.push(PathBuf::from(v)),
                "--translate-storage" => storage = Some(PathBuf::from(v)),
                "--translation-filter" => o.translation_filter.push(PathBuf::from(v)),
                // these three are the environment variables' options: set
                // for this run, they win over the variable and the saved setting
                "--colors" if matches!(v.as_str(), "truecolor" | "24bit" | "256" | "16") => std::env::set_var("EMUERA_COLORS", v),
                "--ambiguous" if matches!(v.as_str(), "wide" | "narrow" | "single" | "osc66") => std::env::set_var("EMUERA_AMBIGUOUS", v),
                "--libretranslate" => std::env::set_var("EMUERA_LIBRETRANSLATE", v),
                _ => usage(),
            }
            continue;
        }
        match a.as_str() {
            "--headless" => o.headless = true,
            "--strict" => o.strict = true,
            "--check" => o.check = true,
            "--game-debug" => o.game_debug = true,
            "--vm-debug" => o.vm_debug = true,
            "--echo" => o.echo = true,
            "--choose" => o.choose = true,
            "--launch-flags" => {
                let path = it.next().unwrap_or_else(|| usage());
                o.launch_flags.push(PathBuf::from(path));
            }
            "--saves" => o.saves = Some("list"),
            "--backup-saves" => o.saves = Some("backup"),
            "--install" | "--uninstall" | "--upgrade" | "--reset-data" => {
                let r = match a.as_str() {
                    "--install" => picker::install(),
                    "--uninstall" => picker::uninstall(),
                    "--upgrade" => picker::upgrade(),
                    _ => picker::reset_data(),
                };
                match r {
                    Ok(m) => {
                        println!("{}", m);
                        std::process::exit(0)
                    }
                    Err(e) => {
                        eprintln!("emuera-tty: {}", e);
                        std::process::exit(1)
                    }
                }
            }
            "--gfx" => o.gfx = it.next().unwrap_or_else(|| usage()),
            "--clone" => o.clone = Some(it.next().unwrap_or_else(|| usage())),
            "--retro" => o.retro = Some("screen".into()),
            s if s.starts_with("--retro=") => o.retro = Some(s[8..].to_string()),
            "--term" => o.term = Some(it.next().unwrap_or_else(|| usage())),
            "--serial" => o.serial = Some(it.next().unwrap_or_else(|| usage())),
            "--baud" => o.baud = it.next().and_then(|v| v.parse().ok()).unwrap_or_else(|| usage()),
            "--serial-format" => o.serial_format = it.next().unwrap_or_else(|| usage()),
            "--flow" => o.flow = it.next().unwrap_or_else(|| usage()),
            "--size" => {
                let v = it.next().unwrap_or_else(|| usage());
                o.size = v.split_once(['x', 'X']).and_then(|(c, r)| Some((c.parse().ok()?, r.parse().ok()?)));
                if o.size.is_none() {
                    usage();
                }
            }
            "-h" | "--help" => usage_exit(0),
            "--manual" | "--man" => {
                picker::show_manual();
                std::process::exit(0)
            }
            s if s.starts_with("--gfx=") => o.gfx = s[6..].to_string(),
            s if s.starts_with("--clone=") => o.clone = Some(s[8..].to_string()),
            s if s.starts_with('-') => usage(),
            s if picker::is_git_url(s) && !std::path::Path::new(s).exists() => o.clone = Some(s.to_string()),
            s => o.game = Some(PathBuf::from(s)),
        }
    }
    if let Ok(g) = std::env::var("EMUERA_GFX") {
        if o.gfx == "auto" {
            o.gfx = g;
        }
    }
    // --translate-storage: its file is read first (so --translate-load files
    // win over it) and is the log unless --translate-log says another
    if let Some(s) = storage {
        o.translate_load.insert(0, s.clone());
        if o.translate_log.is_none() {
            o.translate_log = Some(s);
        }
    }
    // Process launch flags files in order (later files win over earlier)
    let launch_flags = o.launch_flags.to_vec();
    for path in launch_flags {
        parse_launch_flags(&mut o, &path.to_string_lossy());
    }
    // Apply launch flags file values as env vars (they win over saved settings)
    if let Some(c) = &o.colors {
        if matches!(c.as_str(), "truecolor" | "24bit" | "256" | "16") {
            std::env::set_var("EMUERA_COLORS", c);
        }
    }
    if let Some(a) = &o.ambiguous {
        if matches!(a.as_str(), "wide" | "narrow" | "single" | "osc66") {
            std::env::set_var("EMUERA_AMBIGUOUS", a);
        }
    }
    if let Some(l) = &o.libretranslate {
        std::env::set_var("EMUERA_LIBRETRANSLATE", l);
    }
    // Check env vars for debug flags
    if !o.game_debug {
        o.game_debug = std::env::var("EMUERA_GAME_DEBUG").is_ok();
    }
    if !o.vm_debug {
        o.vm_debug = std::env::var("EMUERA_VM_DEBUG").is_ok();
    }
    o
}

/// Read and parse the game's ERH / ERB files (on all cores): the program
/// and how many files there were. Also used by the reload-ERB hotkey.
/// `lazy`: function bodies are compiled when first called (less memory,
/// quicker start); else all now (`--check`).
fn build_program(
    game: &std::path::Path,
    cfg: &std::sync::Arc<emuconfig::Values>,
    erb_dir: Option<&std::path::Path>,
    rename: &[(String, String)],
    debug: bool,
    lazy: bool,
    phase: &mut dyn FnMut(&str),
) -> (parser::Program, usize) {
    // the settings the lexer reads (see expr.rs)
    expr::FULL_SPACE_IS_SPACE.store(cfg.bool("全角スペースをホワイトスペースに含める"), std::sync::atomic::Ordering::Relaxed);
    expr::EXPAND_TRIPLE_SYMBOLS.store(!cfg.bool("FORM中の三連記号を展開しない"), std::sync::atomic::Ordering::Relaxed);
    let mut p = parser::Parser::new(rename, debug, cfg.clone());
    // EMUERA_THREADS=n limits the worker threads (default: all cores)
    let cores = std::env::var("EMUERA_THREADS")
        .ok()
        .and_then(|n| n.parse().ok())
        .filter(|&n: &usize| n > 0)
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
    let (erhs, erbs) = match erb_dir {
        Some(erb) => {
            // Emuera's order (it decides which of a duplicate wins, the
            // order of event functions, and of #DIMs in saves)
            let l = loader::Listing::of(game, cfg);
            let (erhs, erbs) = (loader::emuera_files(erb, &["erh"], l), loader::erb_files(erb, l));
            if !l.subdirs {
                let skipped = loader::walk(erb, &["erb", "erh"]).len().saturating_sub(erhs.len() + erbs.len());
                if skipped > 0 {
                    eprintln!(
                        "{}",
                        t!(
                            "note: {n} ERB/ERH files in subfolders not loaded: the game's config doesn't turn on サブディレクトリを検索する (Search subfolders), so Emuera skips them too",
                            n = skipped
                        )
                    );
                }
            }
            (erhs, erbs)
        }
        None => (Vec::new(), Vec::new()),
    };
    let rel = |f: &std::path::Path| f.strip_prefix(game).unwrap_or(f).display().to_string();
    for f in erhs.iter().chain(&erbs) {
        p.prog.files.push(rel(f));
    }
    // headers first: what they declare holds in every .ERB
    let texts: Vec<Result<String, String>> = parallel_map(&erhs, cores, |f| loader::read_text(f).map_err(|e| format!("{}: {}", f.display(), e)));
    phase("read erh");
    for (i, (f, t)) in erhs.iter().zip(texts).enumerate() {
        match t {
            Ok(text) => p.parse_erh(i as u32, &parser::SourceFile { path: rel(f), text }),
            Err(e) => eprintln!("{}", e),
        }
    }
    phase("parse erh");
    let n_files = erhs.len() + erbs.len() + 1;
    if lazy {
        p.compile_lazily(n_files);
    }
    // ERB files are independent once the headers are known: read and parse
    // them on all cores in contiguous chunks (a file at a time per core),
    // then merge in file order
    let items: Vec<(u32, &PathBuf, usize)> =
        erbs.iter().enumerate().map(|(j, f)| ((erhs.len() + j) as u32, f, std::fs::metadata(f).map_or(1, |m| m.len() as usize))).collect();
    let forks: Vec<parser::Program> = parallel_chunks(
        &items,
        cores,
        |it| it.2,
        |chunk| {
            let mut q = p.fork();
            for (i, f, _) in chunk {
                match std::fs::read(f) {
                    Ok(raw) => q.parse_erb_file(*i, &rel(f), f, &raw),
                    Err(e) => eprintln!("{}: {}", f.display(), e),
                }
            }
            q.prog
        },
    );
    for prog in forks {
        p.absorb(prog);
    }
    // debugging aid: EMUERA_INJECT="STATEMENT||STATEMENT…" becomes a
    // function @__INJECT the VM runs at input number EMUERA_INJECT_AT
    if let Ok(code) = std::env::var("EMUERA_INJECT") {
        let src = parser::SourceFile { path: "(EMUERA_INJECT)".into(), text: format!("@__INJECT\n{}\n", code.replace("||", "\n")) };
        p.prog.files.push(src.path.clone());
        p.parse_erb((n_files - 1) as u32, &src);
    }
    p.prog.funcs.shrink();
    phase("parse erb");
    (p.prog, n_files - 1)
}

/// Write the sampling profile: time per Rust function, by itself and with
/// what it calls.
#[cfg(feature = "profile")]
fn write_pprof(path: &str, g: &pprof::ProfilerGuard) {
    let Ok(r) = g.report().build() else { return };
    let mut own: std::collections::HashMap<String, isize> = Default::default();
    let mut incl: std::collections::HashMap<String, isize> = Default::default();
    let mut edges: std::collections::HashMap<String, isize> = Default::default();
    let mut total = 0isize;
    for (frames, n) in &r.data {
        total += n;
        let names: Vec<String> = frames.frames.iter().flat_map(|f| f.iter().map(|s| s.name())).collect();
        if let Some(l) = names.first() {
            *own.entry(l.clone()).or_default() += n;
        }
        // caller → callee edges (frames run innermost first)
        let mut seen_e = std::collections::HashSet::new();
        for w in names.windows(2) {
            let e = format!("{}  <-  {}", w[0], w[1]);
            if seen_e.insert(e.clone()) {
                *edges.entry(e).or_default() += n;
            }
        }
        let mut seen = std::collections::HashSet::new();
        for nm in names {
            if seen.insert(nm.clone()) {
                *incl.entry(nm).or_default() += n;
            }
        }
    }
    let top = |m: &std::collections::HashMap<String, isize>| {
        let mut v: Vec<_> = m.iter().collect();
        v.sort_by(|a, b| b.1.cmp(a.1));
        v.into_iter().take(80).map(|(k, n)| format!("{:6.2}% {:7} {}\n", *n as f64 * 100.0 / total.max(1) as f64, n, k)).collect::<String>()
    };
    let _ = std::fs::write(path, format!("{} samples\n== self\n{}\n== inclusive\n{}\n== calls\n{}", total, top(&own), top(&incl), top(&edges)));
}

/// "  rss 123 MB" (Linux), for EMUERA_TIMING.
fn rss() -> String {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmRSS:")).and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok()))
        .map_or(String::new(), |kb| format!("  rss {} MB", kb / 1024))
}

/// Load the game and run it; returns the process exit code.
fn run(o: Opts) -> i32 {
    let Some(game) = choose_game(&o) else { return 0 };
    // development builds (--features profile): EMUERA_PPROF=<file>
    #[cfg(feature = "profile")]
    let _pprof = std::env::var("EMUERA_PPROF")
        .ok()
        .map(|f| (f, pprof::ProfilerGuardBuilder::default().frequency(4000).blocklist(&["libc", "libgcc", "pthread", "vdso"]).build().unwrap()));
    if let Some(cmd) = o.saves {
        return saves_cmd(cmd, &game);
    }
    let Some(csv_dir) = loader::find_dir_ci(&game, "CSV") else {
        eprintln!("{}", t!("{dir}: no CSV directory found", dir = game.display()));
        return 1;
    };
    let erb_dir = loader::find_dir_ci(&game, "ERB");
    let t0 = std::time::Instant::now();
    // EMUERA_TIMING=1: report how long each startup phase takes
    let timing = std::env::var_os("EMUERA_TIMING").is_some();
    let mut lap = std::time::Instant::now();
    let mut phase = |name: &str| {
        if timing {
            eprintln!("timing: {:<12} {:>8.2?}{}", name, lap.elapsed(), rss());
        }
        lap = std::time::Instant::now();
    };
    // the game's Emuera settings (CSV/_default.config, emuera.config,
    // CSV/_fixed.config), read once for the loader, parser and VM
    let cfg = std::sync::Arc::new(emuconfig::Values::read(&game));
    let csv = csvdata::GameCsv::load(&csv_dir, erb_dir.as_deref(), &cfg);
    // the code page byte lengths (STRLENS…) are counted in
    loader::ANSI_LANGUAGE.store(loader::ansi_language(cfg.value("内部で使用する東アジア言語")), std::sync::atomic::Ordering::Relaxed);
    // names are always matched whatever their case (doc/differences.md)
    if !cfg.bool("大文字小文字の違いを無視する") || cfg.bool("関数・属性については大文字小文字を無視しない") {
        eprintln!("emuera-tty: {}", t!("this game's settings ask for names that differ only in letter case to be different; emuera-tty always ignores case"));
    }
    // Emuera-Anchor: the parser marks constants as Anchor folds them
    translate::FOLD.store(csv.tr.active(), std::sync::atomic::Ordering::Relaxed);
    translate::PARAM_RENAME.store(csv.tr.active() || csv.param_csv, std::sync::atomic::Ordering::Relaxed);
    phase("csv");
    for w in &csv.warnings {
        eprintln!("csv: {}", w);
    }
    // function bodies are compiled when first called, except for --check
    // (or EMUERA_PARSE=all): a game runs a few percent of them
    let lazy = !o.check && std::env::var("EMUERA_PARSE").map_or(true, |v| v != "all");
    let (prog, n_files) = build_program(&game, &cfg, erb_dir.as_deref(), &csv.rename, o.game_debug, lazy, &mut phase);
    let n_funcs = prog.funcs.len();
    eprintln!(
        "loaded {} files, {} functions, {} chara templates in {:.2?} ({} parse errors)",
        n_files,
        n_funcs,
        csv.charas.len(),
        t0.elapsed(),
        prog.errors.len()
    );
    for e in prog.errors.iter().take(if o.check { usize::MAX } else { 30 }) {
        eprintln!("parse: {}", e);
    }
    if prog.errors.len() > 30 && !o.check {
        eprintln!("... {} more parse errors (run with --check to list all)", prog.errors.len() - 30);
    }
    if o.check {
        if std::env::var_os("EMUERA_LINT").is_some() {
            for l in lint::lint(&prog) {
                eprintln!("lint: {}", l);
            }
        }
        let code = if prog.errors.is_empty() { 0 } else { 1 };
        #[cfg(feature = "profile")]
        if let Some((f, g)) = &_pprof {
            write_pprof(f, g);
        }
        // the process exits next: don't spend time freeing ~100k functions
        std::mem::forget(prog);
        return code;
    }
    // the parse threads leave most of their scratch memory (source text,
    // line copies) cached in glibc's per-thread arenas: give it back
    sys::trim_memory();
    // saved settings (options and EMUERA_* variables win) - load early for warmup
    let st = settings::Settings::load();
    st.export_env();
    let mut res = resources::Resources::load(loader::find_dir_ci(&game, "resources").as_deref());
    // decode the pictures the game used last time while it starts
    // (off in headless runs; "Decode last used pictures" setting controls it)
    let warm = match st.get("warmup") {
        "on" => true,
        "off" => false,
        _ => !o.headless && !o.check, // "auto" or unknown: default behavior
    };
    if warm {
        res.start_warmup(&game);
    }
    // the translation files (see mt.rs) are known before any translation starts
    mt::set_files(o.translate_load.clone(), o.translate_log.clone());
    if let Err(e) = mt::set_filter(&o.translation_filter) {
        eprintln!("{}", e);
        std::process::exit(2);
    }
    let mut o = o;
    if o.gfx == "auto" && std::env::var_os("EMUERA_GFX").is_none() {
        o.gfx = st.get("images").to_string();
    }
    let term_env = std::env::var("TERM").unwrap_or_default();
    if !o.headless && o.serial.is_none() && tty::tmux_passthrough_missing(&o.gfx) {
        eprintln!(
            "emuera-tty: {}",
            t!("inside tmux, pictures in Ghostty or kitty need `set -g allow-passthrough all` (in ~/.tmux.conf, then `tmux source-file ~/.tmux.conf`); `--gfx none` plays without pictures")
        );
        return 1;
    }
    let use_retro = !o.headless
        && (o.serial.is_some() || o.retro.is_some() || st.get("frontend") == "retro" || (st.get("frontend") != "ansi" && retro::wanted_for(&term_env)));
    let fe: Box<dyn frontend::Frontend> = if o.headless {
        Box::new(frontend::Headless::new(o.echo))
    } else if use_retro {
        let port = match &o.serial {
            Some(dev) => retro::Port::serial(dev, &retro::SerialOpts { baud: o.baud, format: o.serial_format.clone(), flow: o.flow.clone() }),
            None => retro::Port::local(),
        };
        let term = o.term.clone().unwrap_or_else(|| if o.serial.is_some() || term_env.is_empty() { "vt100".into() } else { term_env.clone() });
        let r = port.and_then(|p| retro::Retro::new(p, &term, o.size, o.retro.as_deref() == Some("line")));
        match r {
            Ok(r) => {
                if let Some(dev) = &o.serial {
                    eprintln!("emuera-tty: playing on {} ({}, {} baud, {} mode)", dev, r.term_name(), o.baud, r.mode_name());
                }
                Box::new(r)
            }
            Err(e) => {
                eprintln!("emuera-tty: {}", e);
                return 1;
            }
        }
    } else {
        match tty::Tty::new(&o.gfx) {
            Ok(mut t) => {
                t.contrast = st.contrast_ratio();
                t.set_recolor(&std::env::var("EMUERA_RECOLOR").unwrap_or_else(|_| st.get("recolor").to_string()));
                t.wheel = st.get("wheel").parse().unwrap_or(3);
                t.yen_mode = st.get("yen").to_string();
                t.letters_mode = st.get("letters").to_string();
                t.status_info = st.get("statusbar") == "on";
                t.show_memory = st.get("memory") == "on";
                t.set_kbd_pointer(st.get("kbdpointer"));
                t.set_gamepad(st.get("gamepad"));
                if st.get("translator") != "off" {
                    let note = t.set_translate(&st);
                    eprintln!("{}", note);
                }
                t.clip_mode = st.get("clipboard").to_string();
                Box::new(t)
            }
            Err(e) => {
                eprintln!("{}", t!("terminal init failed ({error}); falling back to headless", error = e));
                Box::new(frontend::Headless::new(o.echo))
            }
        }
    };
    // Enable debugger via --vm-debug flag or EMUERA_VM_DEBUG env var
    // The VM reads EMUERA_VM_DEBUG directly
    let rename = csv.rename.clone();
    let mut vm = vm::Vm::new(prog, csv, res, fe, game.clone(), cfg.clone());
    // the reload hotkey parses the ERB files again
    {
        let (g, erb, debug) = (game.clone(), erb_dir.clone(), o.game_debug);
        // (the settings page may have changed the config)
        vm.reloader = Some(Box::new(move || {
            let cfg = std::sync::Arc::new(emuconfig::Values::read(&g));
            build_program(&g, &cfg, erb.as_deref(), &rename, debug, lazy, &mut |_| {}).0
        }));
    }
    if timing {
        eprintln!("timing: {:<12} {:>8.2?}{} (resources, terminal, data init)", "vm ready", lap.elapsed(), rss());
    }
    if let Some(px) = emuconfig::load(&cfg, &mut vm.con, &mut vm.text_exts, &mut vm.save_opts) {
        // no real terminal to fit: Emuera's width, the half-width cells
        // (font_size/2 px) that fit in its drawable width, the window less
        // max(2, font_size/6) px (Config.DrawableWidth, getStBar: 940 px at
        // 16 is 117, which eratohoreverse's title checks for)
        if o.headless {
            let fs = vm.con.font_size;
            let shift = ((fs as i64) / 6).max(2) as f32;
            let n = ((px - shift) / (fs / 2.0).max(1.0)) as usize;
            // (cols less one is the width, as on a terminal)
            vm.fe.set_cols(n + 1);
            vm.con.width = n.max(20);
        }
    }
    vm.strict = o.strict;
    // --translate-log, --translation-filter files: lines remember where they were printed from
    if o.translate_log.is_some() || mt::filter_has_files() {
        vm.con.track_src = true;
        vm.con.src_files = vm.prog.files.clone();
    }
    // sound: in a terminal, through a player program (see sound.rs);
    // EMUERA_SOUND=on/off and EMUERA_SOUND_PLAYER=mpv win over the setting,
    // which the settings page can change while playing
    if !o.headless {
        vm.con.sound_on = sound::wanted(st.get("sound"));
        let named = std::env::var("EMUERA_SOUND_PLAYER").ok();
        vm.sound = sound::Sound::new(sound::Player::find(named.as_deref()), sound::Player::find_midi(&game));
        if let Some(s) = vm.sound.as_mut() {
            s.set_on(vm.con.sound_on);
        }
        // a game that comes with eraMidi (eratohoReverse): its music, found
        // in the printed lines as eraMidi finds it (sound::eramidi_song)
        vm.con.note_watch = vm.sound.is_some() && loader::find_file_ci(&game, "eramidi.exe").is_some();
    }
    let sd = vm.save_dir();
    vm.fe.set_game(&game, &sd);
    let r = vm.run();
    vm.fe.shutdown();
    #[cfg(feature = "profile")]
    if let Some((f, g)) = &_pprof {
        write_pprof(f, g);
    }
    if vm.defer_warnings {
        for w in &vm.log {
            eprintln!("warning: {}", w);
        }
    }
    for w in vm.res.warnings.iter().take(10) {
        eprintln!("resources: {}", w);
    }
    // the process exits next; freeing the program and all game data one
    // allocation at a time would only delay quitting
    std::mem::forget(vm);
    match r {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{}", e);
            1
        }
    }
}

/// Run `f` over contiguous chunks of `items` on up to `threads` threads and
/// return the results in chunk order. Chunks are cut so each holds about
/// the same `weight` (e.g. bytes of source), several per thread, and idle
/// threads take the next chunk from a shared counter, so one huge file
/// doesn't leave the other cores waiting. Threads get a big stack
/// (parsing recurses).
fn parallel_chunks<T: Sync, R: Send>(items: &[T], threads: usize, weight: impl Fn(&T) -> usize, f: impl Fn(&[T]) -> R + Sync) -> Vec<R> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    if items.is_empty() {
        return Vec::new();
    }
    let threads = threads.max(1);
    // cut into ~4 chunks per thread of similar total weight
    let total: usize = items.iter().map(&weight).sum::<usize>().max(1);
    let target = total.div_ceil(threads * 4).max(1);
    let mut bounds = vec![0];
    let mut acc = 0;
    for (i, it) in items.iter().enumerate() {
        acc += weight(it);
        if acc >= target && i + 1 < items.len() {
            bounds.push(i + 1);
            acc = 0;
        }
    }
    bounds.push(items.len());
    let chunks: Vec<&[T]> = bounds.windows(2).map(|w| &items[w[0]..w[1]]).collect();
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<R>>> = Mutex::new((0..chunks.len()).map(|_| None).collect());
    std::thread::scope(|s| {
        let work = || loop {
            let k = next.fetch_add(1, Ordering::Relaxed);
            let Some(c) = chunks.get(k) else { break };
            let r = f(c);
            results.lock().unwrap()[k] = Some(r);
        };
        for _ in 0..threads.min(chunks.len()) {
            stack_sizes(64 << 20).find_map(|size| std::thread::Builder::new().stack_size(size).spawn_scoped(s, work).ok()).expect("spawn");
        }
    });
    results.into_inner().unwrap().into_iter().map(|r| r.expect("chunk not processed")).collect()
}

/// `f` applied to every item, using up to `threads` threads; order kept.
fn parallel_map<T: Sync, R: Send>(items: &[T], threads: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    parallel_chunks(items, threads, |_| 1, |c| c.iter().map(&f).collect::<Vec<R>>()).into_iter().flatten().collect()
}

/// `--saves` / `--backup-saves`.
fn saves_cmd(cmd: &str, game: &std::path::Path) -> i32 {
    let dir = savemgr::save_dir_of(game);
    let root = savemgr::backup_root(game).unwrap_or_else(|| dir.join("backups"));
    if cmd == "backup" {
        return match savemgr::backup_all(&dir, &root, "") {
            Ok((p, n)) => {
                println!("{}", t!("backed up {n} files to {dir}", n = n, dir = p.display()));
                0
            }
            Err(e) => {
                eprintln!("emuera-tty: {}", e);
                1
            }
        };
    }
    println!("{}", t!("saves in {dir}:", dir = dir.display()));
    for s in savemgr::list(&dir) {
        println!("  {} {} {:>9}  {}", i18n::pad(&s.label(), 8), i18n::pad(&savemgr::when(s.modified), 16), s.size, s.comment);
    }
    let bks = savemgr::backups(&root);
    if !bks.is_empty() {
        println!("{}", t!("{n} backups in {dir}", n = bks.len(), dir = root.display()));
    }
    0
}

/// Decide which game folder to run (see `usage`). None: the user cancelled
/// the picker (or there is nothing to run, already reported).
fn choose_game(o: &Opts) -> Option<PathBuf> {
    let interactive = !o.headless && !o.check && sys::is_tty();
    // a repository to clone: the launcher clones it and selects the game
    if let Some(url) = &o.clone {
        if !interactive {
            eprintln!("emuera-tty: cloning {} needs a terminal (the launcher does it)", url);
            return None;
        }
        let g = picker::launcher(Some(url), o.game_debug, o.vm_debug)?;
        picker::remember(&g);
        return Some(g);
    }
    // an explicit folder is used as-is; only picker choices are remembered
    if let Some(g) = &o.game {
        if archive::is_archive(g) {
            if let Some(dir) = archive::already_extracted(g) {
                return Some(dir);
            }
            eprint!("{}", t!("Extracting {archive}…", archive = g.display()));
            let r = archive::extract_game(g, &mut |n| eprint!("\r{}", t!("Extracting {archive}… {n} files", archive = g.display(), n = n)));
            eprintln!();
            return match r {
                Ok(dir) => {
                    eprintln!("{}", t!("Game folder: {dir}", dir = dir.display()));
                    Some(dir)
                }
                Err(e) => {
                    eprintln!("emuera-tty: {}", e);
                    None
                }
            };
        }
        // a game played by path joins the library too (not as the last one)
        if interactive && picker::is_game_dir(g) && !picker::in_testing_mode() {
            let mut st = settings::Settings::load();
            st.add_game(g);
            st.save();
        }
        return Some(g.clone());
    }
    if !o.choose {
        if picker::is_game_dir(std::path::Path::new(".")) {
            return Some(PathBuf::from("."));
        }
        // one game known: play it; several: let the user choose
        if picker::library_size() < 2 || !interactive {
            if let Some(g) = picker::last_game() {
                return Some(g);
            }
        }
    }
    if !interactive {
        eprintln!("emuera-tty: {}", t!("no game folder given (pass GAME_DIR, or run in a terminal to pick one)"));
        return None;
    }
    let g = picker::launcher(None, o.game_debug, o.vm_debug)?;
    picker::remember(&g);
    Some(g)
}

/// Stack sizes to try for a thread that wants `want` bytes, largest first.
/// Some systems cap a thread's stack (Haiku: 16 MB less its guard pages),
/// so each halving is tried 64 KB short too.
fn stack_sizes(want: usize) -> impl Iterator<Item = usize> {
    std::iter::successors(Some(want), |&s| (s > 1 << 20).then_some(s / 2)).flat_map(|s| [s, s - (64 << 10)])
}

fn main() {
    // the interface language, before anything is printed
    i18n::select(settings::Settings::load().get("language"));
    let o = parse_args();
    // ERB call chains can be deep: run on a thread with a large stack
    // (EMUERA_STACK_MB, default 512), or the largest the system allows
    let want = std::env::var("EMUERA_STACK_MB").ok().and_then(|m| m.parse::<usize>().ok()).filter(|&m| m > 0).map_or(512 << 20, |m| m << 20);
    let o = std::sync::Arc::new(std::sync::Mutex::new(Some(o)));
    let h = stack_sizes(want)
        .find_map(|size| {
            let o = o.clone();
            let h = std::thread::Builder::new().stack_size(size).spawn(move || run(o.lock().unwrap().take().expect("options"))).ok()?;
            vm::set_stack_bytes(size);
            Some(h)
        })
        .expect("spawn");
    let code = h.join().unwrap_or(101);
    std::process::exit(code);
}
