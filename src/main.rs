//! emuera-tty: run Emuera (ERB) games in a terminal.
//!
//! Startup: parse options, read CSVs (`csvdata`), parse every .ERH then
//! .ERB file (`parser`), load image resources, pick a frontend (full-screen
//! `tty` or `--headless`), read the game's config files, and hand over to
//! the VM's system loop on a thread with a large stack (ERB code can
//! recurse deeply, and each ERB call is a Rust call).

mod archive;
mod ast;
mod config_names;
mod console;
mod csvdata;
mod expr;
mod frontend;
mod fxhash;
mod gfx;
mod html;
mod loader;
mod mice;
mod parser;
mod picker;
mod regex;
mod resources;
mod retro;
mod settings;
mod terminfo;
mod tty;
mod xml;
mod vm;

use std::path::PathBuf;

/// Command-line options.
struct Opts {
    /// None: use the current folder, the remembered game, or the picker
    game: Option<PathBuf>,
    /// --choose: open the picker even if a game is remembered
    choose: bool,
    headless: bool,
    gfx: String,
    strict: bool,
    check: bool,
    debug: bool,
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
}

/// Print help; exit 0 when asked for (--help), 2 on bad options.
fn usage_exit(code: i32) -> ! {
    eprintln!(
        "usage: emuera-tty [GAME_DIR] [options]\n\
         \n\
         GAME_DIR may be a game folder or an archive of one (.zip, .tar.gz, .7z…;\n\
         extracted once to ~/.local/share/emuera-tty/games).\n\
         Without GAME_DIR: the current folder if it is a game, else the game\n\
         chosen last time, else a folder picker opens.\n\
         \n\
         options:\n\
         \x20 --choose                         pick a game folder (even if one is remembered)\n\
         \x20 --gfx auto|kitty|sixel|fb|none   image backend (default auto)\n\
         \x20 --headless                       plain stdin/stdout mode (scripts, tests)\n\
         \x20 --echo                           echo inputs in headless mode\n\
         \x20 --check                          parse the game and report errors only\n\
         \x20 --strict                         unknown commands are errors\n\
         \x20 --debug                          enable [IF_DEBUG] / ;#; lines\n\
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
         \x20 --reset-data                     forget settings and the game library; saves are kept"
    );
    std::process::exit(code)
}

fn usage() -> ! {
    usage_exit(2)
}

/// Parse `std::env::args` (exits with usage on unknown options).
fn parse_args() -> Opts {
    let mut o = Opts {
        game: None,
        choose: false,
        headless: false,
        gfx: "auto".into(),
        strict: false,
        check: false,
        debug: false,
        echo: false,
        retro: None,
        term: None,
        serial: None,
        baud: 9600,
        serial_format: "8N1".into(),
        flow: "xon".into(),
        size: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--headless" => o.headless = true,
            "--strict" => o.strict = true,
            "--check" => o.check = true,
            "--debug" => o.debug = true,
            "--echo" => o.echo = true,
            "--choose" => o.choose = true,
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
            s if s.starts_with("--gfx=") => o.gfx = s[6..].to_string(),
            s if s.starts_with('-') => usage(),
            s => o.game = Some(PathBuf::from(s)),
        }
    }
    if let Ok(g) = std::env::var("EMUERA_GFX") {
        if o.gfx == "auto" {
            o.gfx = g;
        }
    }
    o
}

/// Read a few keys from the config files (key:value lines), in Emuera's
/// order: CSV/_default.config, emuera.config, CSV/_fixed.config (wins).
/// Returns the configured window width in pixels, if any.
fn load_config(game: &std::path::Path, csv_dir: &std::path::Path, con: &mut console::Console, text_exts: &mut Vec<String>, saves: &mut vm::save::SaveOpts) -> Option<f32> {
    let mut paths = vec![loader::find_file_ci(csv_dir, "_default.config"), loader::find_file_ci(game, "emuera.config")];
    if let Some(d) = loader::find_dir_ci(game, "config") {
        paths.push(loader::find_file_ci(&d, "emuera.config"));
    }
    paths.push(loader::find_file_ci(csv_dir, "_fixed.config"));
    let mut window_px = None;
    let yes = |v: &str| v.eq_ignore_ascii_case("YES") || v.eq_ignore_ascii_case("TRUE");
    let mut zip = false;
    let mut binary = false;
    for p in paths.into_iter().flatten() {
        let Ok(text) = loader::read_text(&p) else { continue };
        for line in text.lines() {
            let Some((k, v)) = line.split_once(':') else { continue };
            let v = v.trim();
            let rgb = |v: &str| -> Option<console::Rgb> {
                let n: Vec<u8> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                if n.len() == 3 { Some(console::Rgb(n[0], n[1], n[2])) } else { None }
            };
            // keys may be written in Japanese or English (WINDOW WIDTH…)
            let k = line.trim_start_matches('\u{feff}').split_once(':').map(|x| x.0).unwrap_or(k);
            match config_names::canonical(k) {
                "ウィンドウ幅" => window_px = v.parse().ok().or(window_px),
                "PRINTCを並べる数" => con.printc_per_line = v.parse().unwrap_or(con.printc_per_line),
                "フォント名" => {
                    con.default_font = v.to_string();
                    con.font_name = v.to_string();
                }
                "ボタンの途中で行を折りかえさない" => con.button_wrap = yes(v),
                "セーブデータをsavフォルダ内に作成する" => saves.in_folder = yes(v),
                "セーブデータをバイナリ形式で保存する" => binary = yes(v),
                "セーブデータを圧縮して保存する" => zip = yes(v),
                "オートセーブを行なう" => saves.autosave = yes(v),
                "LOADTEXTとSAVETEXTで使える拡張子" => {
                    for e in v.split(',').map(|e| e.trim().trim_start_matches('.').to_lowercase()).filter(|e| !e.is_empty()) {
                        if !text_exts.contains(&e) {
                            text_exts.push(e);
                        }
                    }
                }
                "フォントサイズ" => con.font_size = v.parse().unwrap_or(con.font_size),
                "一行の高さ" => con.line_height = v.parse().unwrap_or(con.line_height),
                "文字色" => {
                    if let Some(c) = rgb(v) {
                        con.default_fg = c;
                        con.style.fg = c;
                    }
                }
                "背景色" => {
                    if let Some(c) = rgb(v) {
                        con.default_bg = c;
                        con.screen_bg = c;
                    }
                }
                "選択中文字色" => {
                    if let Some(c) = rgb(v) {
                        con.focus_fg = c
                    }
                }
                "履歴文字色" => {
                    if let Some(c) = rgb(v) {
                        con.log_fg = c
                    }
                }
                "PRINTCを並べる文字数" | "PRINTCの文字数" => con.printc_len = v.parse().unwrap_or(con.printc_len),
                "表示するログの最大行数" | "履歴ログの行数" => con.max_lines = v.parse().unwrap_or(con.max_lines),
                _ => {}
            }
        }
    }
    // Emuera compresses only binary saves; emuera-tty always writes binary
    // (Emuera reads it whatever its config says)
    saves.zip = zip && binary;
    window_px
}

/// Load the game and run it; returns the process exit code.
fn run(o: Opts) -> i32 {
    let Some(game) = choose_game(&o) else { return 0 };
    let Some(csv_dir) = loader::find_dir_ci(&game, "CSV") else {
        eprintln!("{}: no CSV directory found", game.display());
        return 1;
    };
    let erb_dir = loader::find_dir_ci(&game, "ERB");
    let t0 = std::time::Instant::now();
    // EMUERA_TIMING=1: report how long each startup phase takes
    let timing = std::env::var_os("EMUERA_TIMING").is_some();
    let mut lap = std::time::Instant::now();
    let mut phase = |name: &str| {
        if timing {
            eprintln!("timing: {:<12} {:>8.2?}", name, lap.elapsed());
        }
        lap = std::time::Instant::now();
    };
    let csv = csvdata::GameCsv::load(&csv_dir, erb_dir.as_deref());
    phase("csv");
    for w in &csv.warnings {
        eprintln!("csv: {}", w);
    }
    let mut p = parser::Parser::new(&csv.rename, o.debug);
    // EMUERA_THREADS=n limits the worker threads (default: all cores)
    let cores = std::env::var("EMUERA_THREADS").ok().and_then(|n| n.parse().ok()).filter(|&n: &usize| n > 0).unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
    let mut files = Vec::new();
    if let Some(erb) = &erb_dir {
        let paths: Vec<std::path::PathBuf> = loader::walk(erb, &["erh"]).into_iter().chain(loader::walk(erb, &["erb"])).collect();
        // read + decode (Shift_JIS detection) in parallel, keeping order
        let read: Vec<Result<String, String>> = parallel_map(&paths, cores, |f| loader::read_text(f).map_err(|e| format!("{}: {}", f.display(), e)));
        for (f, r) in paths.iter().zip(read) {
            match r {
                Ok(text) => files.push(parser::SourceFile { path: f.strip_prefix(&game).unwrap_or(f).display().to_string(), text }),
                Err(e) => eprintln!("{}", e),
            }
        }
    }
    phase("read files");
    let n_erh = files.iter().filter(|f| f.path.to_lowercase().ends_with(".erh")).count();
    for (i, f) in files.iter().enumerate() {
        p.prog.files.push(f.path.clone());
        if i < n_erh {
            p.parse_erh(i as u32, f);
        }
    }
    phase("parse erh");
    // ERB files are independent once the headers are known: parse them on
    // all cores in contiguous chunks, then merge in file order
    let erbs: Vec<(usize, &parser::SourceFile)> = files.iter().enumerate().skip(n_erh).collect();
    let forks: Vec<parser::Program> = parallel_chunks(&erbs, cores, |(_, f)| f.text.len(), |chunk| {
        let mut q = p.fork();
        for (i, f) in chunk {
            q.parse_erb(*i as u32, f);
        }
        q.prog
    });
    for prog in forks {
        p.absorb(prog);
    }
    phase("parse erb");
    let prog = p.prog;
    let n_funcs: usize = prog.funcs.values().map(|v| v.len()).sum();
    eprintln!(
        "loaded {} files, {} functions, {} chara templates in {:.2?} ({} parse errors)",
        files.len(),
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
        let code = if prog.errors.is_empty() { 0 } else { 1 };
        // the process exits next: don't spend time freeing ~100k functions
        std::mem::forget(prog);
        return code;
    }
    drop(files);
    // the parse threads leave most of their scratch memory (source text,
    // line copies) cached in glibc's per-thread arenas: give it back
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        libc::malloc_trim(0);
    }
    let res = resources::Resources::load(loader::find_dir_ci(&game, "resources").as_deref());
    // saved settings (options and EMUERA_* variables win)
    let st = settings::Settings::load();
    st.export_env();
    let mut o = o;
    if o.gfx == "auto" && std::env::var_os("EMUERA_GFX").is_none() {
        o.gfx = st.get("images").to_string();
    }
    let term_env = std::env::var("TERM").unwrap_or_default();
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
                t.wheel = st.get("wheel").parse().unwrap_or(3);
                Box::new(t)
            }
            Err(e) => {
                eprintln!("terminal init failed ({}); falling back to headless", e);
                Box::new(frontend::Headless::new(o.echo))
            }
        }
    };
    let mut vm = vm::Vm::new(prog, csv, res, fe, game.clone());
    if timing {
        eprintln!("timing: {:<12} {:>8.2?} (resources, terminal, data init)", "vm ready", lap.elapsed());
    }
    if let Some(px) = load_config(&game, &csv_dir, &mut vm.con, &mut vm.text_exts, &mut vm.save_opts) {
        // no real terminal to fit: use Emuera's width (half-width cells are font_size/2 px)
        if o.headless {
            vm.fe.set_cols((px / (vm.con.font_size / 2.0).max(1.0)) as usize);
            vm.con.width = vm.fe.cols().saturating_sub(1).max(20);
        }
    }
    vm.strict = o.strict;
    let r = vm.run();
    vm.fe.shutdown();
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
        for _ in 0..threads.min(chunks.len()) {
            std::thread::Builder::new()
                .stack_size(64 << 20)
                .spawn_scoped(s, || loop {
                    let k = next.fetch_add(1, Ordering::Relaxed);
                    let Some(c) = chunks.get(k) else { break };
                    let r = f(c);
                    results.lock().unwrap()[k] = Some(r);
                })
                .expect("spawn");
        }
    });
    results.into_inner().unwrap().into_iter().map(|r| r.expect("chunk not processed")).collect()
}

/// `f` applied to every item, using up to `threads` threads; order kept.
fn parallel_map<T: Sync, R: Send>(items: &[T], threads: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    parallel_chunks(items, threads, |_| 1, |c| c.iter().map(&f).collect::<Vec<R>>()).into_iter().flatten().collect()
}

/// Decide which game folder to run (see `usage`). None: the user cancelled
/// the picker (or there is nothing to run, already reported).
fn choose_game(o: &Opts) -> Option<PathBuf> {
    let interactive = !o.headless && !o.check && unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 };
    // an explicit folder is used as-is; only picker choices are remembered
    if let Some(g) = &o.game {
        if archive::is_archive(g) {
            if let Some(dir) = archive::already_extracted(g) {
                return Some(dir);
            }
            eprint!("Extracting {}…", g.display());
            let r = archive::extract_game(g, &mut |n| eprint!("\rExtracting {}… {} files", g.display(), n));
            eprintln!();
            return match r {
                Ok(dir) => {
                    eprintln!("Game folder: {}", dir.display());
                    Some(dir)
                }
                Err(e) => {
                    eprintln!("emuera-tty: {}", e);
                    None
                }
            };
        }
        // a game played by path joins the library too (not as the last one)
        if interactive && picker::is_game_dir(g) {
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
        eprintln!("emuera-tty: no game folder given (pass GAME_DIR, or run in a terminal to pick one)");
        return None;
    }
    let g = picker::launcher()?;
    picker::remember(&g);
    Some(g)
}

fn main() {
    let o = parse_args();
    // ERB call chains can be deep; run on a thread with a large stack.
    let h = std::thread::Builder::new().stack_size(512 << 20).spawn(move || run(o)).expect("spawn");
    let code = h.join().unwrap_or(101);
    std::process::exit(code);
}
