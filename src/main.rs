mod ast;
mod console;
mod csvdata;
mod expr;
mod frontend;
mod gfx;
mod html;
mod loader;
mod parser;
mod resources;
mod tty;
mod xml;
mod vm;

use std::path::PathBuf;

struct Opts {
    game: PathBuf,
    headless: bool,
    gfx: String,
    strict: bool,
    check: bool,
    debug: bool,
    echo: bool,
}

fn usage() -> ! {
    eprintln!(
        "usage: emuera-rs [GAME_DIR] [options]\n\
         \n\
         options:\n\
         \x20 --gfx auto|kitty|sixel|fb|none   image backend (default auto)\n\
         \x20 --headless                       plain stdin/stdout mode (scripts, tests)\n\
         \x20 --echo                           echo inputs in headless mode\n\
         \x20 --check                          parse the game and report errors only\n\
         \x20 --strict                         unknown commands are errors\n\
         \x20 --debug                          enable [IF_DEBUG] / ;#; lines"
    );
    std::process::exit(2)
}

fn parse_args() -> Opts {
    let mut o = Opts { game: PathBuf::from("."), headless: false, gfx: "auto".into(), strict: false, check: false, debug: false, echo: false };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--headless" => o.headless = true,
            "--strict" => o.strict = true,
            "--check" => o.check = true,
            "--debug" => o.debug = true,
            "--echo" => o.echo = true,
            "--gfx" => o.gfx = it.next().unwrap_or_else(|| usage()),
            "-h" | "--help" => usage(),
            s if s.starts_with("--gfx=") => o.gfx = s[6..].to_string(),
            s if s.starts_with('-') => usage(),
            s => o.game = PathBuf::from(s),
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

fn run(o: Opts) -> i32 {
    let game = o.game.clone();
    let Some(csv_dir) = loader::find_dir_ci(&game, "CSV") else {
        eprintln!("{}: no CSV directory found", game.display());
        return 1;
    };
    let erb_dir = loader::find_dir_ci(&game, "ERB");
    let t0 = std::time::Instant::now();
    let csv = csvdata::GameCsv::load(&csv_dir, erb_dir.as_deref());
    for w in &csv.warnings {
        eprintln!("csv: {}", w);
    }
    let mut p = parser::Parser::new(&csv.rename, o.debug);
    let mut files = Vec::new();
    if let Some(erb) = &erb_dir {
        let erh = loader::walk(erb, &["erh"]);
        let erbs = loader::walk(erb, &["erb"]);
        for f in erh.iter().chain(erbs.iter()) {
            match loader::read_text(f) {
                Ok(text) => files.push(parser::SourceFile { path: f.strip_prefix(&game).unwrap_or(f).display().to_string(), text }),
                Err(e) => eprintln!("{}: {}", f.display(), e),
            }
        }
    }
    let n_erh = files.iter().filter(|f| f.path.to_lowercase().ends_with(".erh")).count();
    for (i, f) in files.iter().enumerate() {
        p.prog.files.push(f.path.clone());
        if i < n_erh {
            p.parse_erh(i as u32, f);
        }
    }
    for (i, f) in files.iter().enumerate().skip(n_erh) {
        p.parse_erb(i as u32, f);
    }
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

fn main() {
    let o = parse_args();
    // ERB call chains can be deep; run on a thread with a large stack.
    let h = std::thread::Builder::new().stack_size(512 << 20).spawn(move || run(o)).expect("spawn");
    let code = h.join().unwrap_or(101);
    std::process::exit(code);
}
