//! emuera-tty: run Emuera (ERB) games in a terminal.
//!
//! Startup: parse options, read CSVs (`csvdata`), parse every .ERH then
//! .ERB file (`parser`), load image resources, pick a frontend (full-screen
//! `tty` or `--headless`), read the game's config files, and hand over to
//! the VM's system loop on a thread with a large stack (ERB code can
//! recurse deeply, and each ERB call is a Rust call).

mod archive;
mod ast;
mod console;
mod csvdata;
mod expr;
mod frontend;
mod gfx;
mod html;
mod loader;
mod mice;
mod parser;
mod picker;
mod resources;
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
         setup:\n\
         \x20 --install                        copy this program to ~/.local/bin (or $XDG_BIN_HOME)\n\
         \x20 --upgrade                        fetch, build (git + cargo) and install the latest version\n\
         \x20 --uninstall                      remove the installed copy\n\
         \x20 --reset-data                     forget settings (remembered game folder); saves are kept"
    );
    std::process::exit(code)
}

fn usage() -> ! {
    usage_exit(2)
}

/// Parse `std::env::args` (exits with usage on unknown options).
fn parse_args() -> Opts {
    let mut o = Opts { game: None, choose: false, headless: false, gfx: "auto".into(), strict: false, check: false, debug: false, echo: false };
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
fn load_config(game: &std::path::Path, csv_dir: &std::path::Path, con: &mut console::Console, text_exts: &mut Vec<String>) -> Option<f32> {
    let mut paths = vec![loader::find_file_ci(csv_dir, "_default.config"), loader::find_file_ci(game, "emuera.config")];
    if let Some(d) = loader::find_dir_ci(game, "config") {
        paths.push(loader::find_file_ci(&d, "emuera.config"));
    }
    paths.push(loader::find_file_ci(csv_dir, "_fixed.config"));
    let mut window_px = None;
    for p in paths.into_iter().flatten() {
        let Ok(text) = loader::read_text(&p) else { continue };
        for line in text.lines() {
            let Some((k, v)) = line.split_once(':') else { continue };
            let v = v.trim();
            let rgb = |v: &str| -> Option<console::Rgb> {
                let n: Vec<u8> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                if n.len() == 3 { Some(console::Rgb(n[0], n[1], n[2])) } else { None }
            };
            match k.trim() {
                "ウィンドウ幅" => window_px = v.parse().ok().or(window_px),
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
                        con.default_bg = c
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
        return if prog.errors.is_empty() { 0 } else { 1 };
    }
    drop(files);
    let res = resources::Resources::load(loader::find_dir_ci(&game, "resources").as_deref());
    let fe: Box<dyn frontend::Frontend> = if o.headless {
        Box::new(frontend::Headless::new(o.echo))
    } else {
        match tty::Tty::new(&o.gfx) {
            Ok(t) => Box::new(t),
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
    if let Some(px) = load_config(&game, &csv_dir, &mut vm.con, &mut vm.text_exts) {
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
        return Some(g.clone());
    }
    if !o.choose {
        if picker::is_game_dir(std::path::Path::new(".")) {
            return Some(PathBuf::from("."));
        }
        if let Some(g) = picker::last_game() {
            return Some(g);
        }
    }
    if !interactive {
        eprintln!("emuera-tty: no game folder given (pass GAME_DIR, or run in a terminal to pick one)");
        return None;
    }
    // start where the last game was, else in the home folder
    let start = picker::last_game()
        .and_then(|g| g.parent().map(|p| p.to_path_buf()))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    let g = picker::pick(&start)?;
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
