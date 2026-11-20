//! The Android frontend: the bridge between the VM thread and the Kotlin UI.
//!
//! The VM runs on its own thread with a big stack and blocks in `Frontend::input`, so
//! it never touches the UI directly. Each update lays lines out into rows of cells and
//! sends them as one byte array, which Kotlin parses and draws. Answers come back
//! through a channel.
//!
//! Replay mode plays a recorded session with no player. It computes the same output hash
//! as bench/erbbench + the patched `Headless`, so a run on the real Android allocator
//! with real drawing can be checked byte for byte against the headless reference. Time
//! is split into what the script took, preparing the update, and waiting for the frame.

use crate::ast::Align;
use crate::console::{is_tip_area, Console, Line, Seg};
use crate::frontend::{raw_from_input, Frontend, Hotkey, InputKind, InputReq, InputResult, RawInput};
use crate::resources::{CbgSrc, Resources};
use crate::vm::value::Value;
use jni::objects::{GlobalRef, JClass, JString, JValue};
use jni::sys::{jboolean, jint, jlong};
use jni::{JNIEnv, JavaVM};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

enum Msg {
    Click(String),
    Line(String),
    Skip,
    Quit,
    Drawn {
        parse_us: i64,
        draw_us: i64,
    },
    /// A tap (`btn` 0) or a long press (2, a touch screen's right click).
    Tap {
        btn: u8,
        col: usize,
        rows_up: usize,
        button: Option<String>,
    },
    Wheel {
        up: bool,
        col: usize,
        rows_up: usize,
    },
    /// Android wants memory back.
    Trim,
    /// The player's text size, as a percentage of the game's own.
    TextSize(usize),
    /// The view changed width: how many columns now fit at the app's smallest text.
    MaxCols(usize),
    /// From the game's menu: back to the title, or read the game's settings again.
    Hotkey(Hotkey),
}

/// How many of Emuera's rows fit in the view at picture scale: what CLIENTHEIGHT says.
static VIEW_ROWS: AtomicUsize = AtomicUsize::new(30);

/// Where the finger is or last was, for what scripts ask between inputs (MOUSEX / MOUSEY,
/// MOUSEB, GETKEY). `rows_up` counts Emuera's rows up from the bottom at picture scale, so
/// that pictures and button maps are hit where they're drawn.
// shortcut: this view packs text rows tighter than Emuera's, so a script turning MOUSEY into a text row drifts higher up the screen; scale the two alike if a game needs that.
struct Pointer {
    touched: bool,
    col: usize,
    rows_up: usize,
    button: String,
    down: bool,
    left_hit: bool,
    right_hit: bool,
}

static POINTER: Mutex<Pointer> =
    Mutex::new(Pointer { touched: false, col: 0, rows_up: 0, button: String::new(), down: false, left_hit: false, right_hit: false });

struct Shared {
    vm: JavaVM,
    class: GlobalRef,
    tx: Sender<Msg>,
}

static SHARED: OnceLock<Shared> = OnceLock::new();
static QUIT: AtomicBool = AtomicBool::new(false);
/// The running game's folder and the save folder its VM uses (`set_game`), for the save manager.
static GAME: Mutex<Option<(PathBuf, PathBuf)>> = Mutex::new(None);

/// Runs `f` with this thread's JNI environment and the Kotlin `Native` class. A local
/// frame stops references piling up: this thread rarely returns to Java.
fn with_java(f: impl FnOnce(&mut JNIEnv, &JClass)) {
    let Some(sh) = SHARED.get() else { return };
    let Ok(mut env) = sh.vm.attach_current_thread_permanently() else { return };
    let class: &JClass = sh.class.as_obj().into();
    let _ = env.with_local_frame(16, |env| -> jni::errors::Result<()> {
        f(env, class);
        Ok(())
    });
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
}

/// Plays a sound `times` times, or for 0 as the looping BGM (see `sound.rs`, `GameAudio.kt`).
pub(crate) fn play_audio(f: &std::path::Path, volume: u8, times: u32) {
    with_java(|env, class| {
        if let Ok(path) = env.new_string(f.to_string_lossy()) {
            let _ = env.call_static_method(
                class,
                "onPlay",
                "(Ljava/lang/String;II)V",
                &[JValue::Object(&path), JValue::Int(volume as i32), JValue::Int(times as i32)],
            );
        }
    });
}

pub(crate) fn stop_audio(bgm: bool) {
    with_java(|env, class| {
        let _ = env.call_static_method(class, "onStopAudio", "(Z)V", &[JValue::Bool(bgm as u8)]);
    });
}

pub(crate) fn set_audio_volumes(se: u8, bgm: u8) {
    with_java(|env, class| {
        let _ = env.call_static_method(class, "onVolumes", "(II)V", &[JValue::Int(se as i32), JValue::Int(bgm as i32)]);
    });
}

fn hash(mut h: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
const HASH_START: u64 = 0xcbf29ce484222325;

fn proc_status(key: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with(key)).and_then(|l| l.split_whitespace().nth(1)?.parse().ok()))
        .unwrap_or(0)
}

/// A recorded session being replayed.
struct Replay {
    answers: VecDeque<String>,
    hash: u64,
    step_hash: u64,
    hashes: String,
    csv: String,
    vm: Vec<u64>,
    started: Instant,
    first_input_ms: u128,
    /// time spent on updates since the last answer (WAITs send updates too)
    prep_us: u64,
    prep_total: u64,
    ack_total: u64,
    parse_total: i64,
    draw_total: i64,
    late_frames: u32,
    dir: PathBuf,
}

impl Replay {
    fn print(&mut self, bytes: &[u8]) {
        self.hash = hash(self.hash, bytes);
        self.step_hash = hash(self.step_hash, bytes);
    }
}

/// One drawn piece of a row. `dy`: rows below the line's row (HTML `<div>` boxes).
struct Run {
    col: i32,
    cells: i32,
    dy: i32,
    kind: u8,
    flags: u8,
    fg: i32,
    bg: i32,
    text: String,
    btn: Option<String>,
    key: i32,
    w: f32,
    h: f32,
    ypos: f32,
}

const NARROW: u8 = 0;
const WIDE: u8 = 1;
const SINGLE: u8 = 2;
const IMAGE: u8 = 3;
const RECT: u8 = 4;
const BOX: u8 = 5;

/// The input kind `GameView` hands every tap to the script for (INPUTMOUSEKEY).
const RAW: u8 = 4;

/// How many of Emuera's half-width cells a character takes, and how it can be drawn:
/// the width rule of tty.rs' `char_cells`, a Shift-JIS byte per cell.
fn char_class(ch: char) -> (i32, u8) {
    if ch.is_ascii() {
        return if ch.is_ascii_control() { (0, NARROW) } else { (1, NARROW) };
    }
    let tw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0) as i32;
    if tw == 0 {
        return (0, SINGLE);
    }
    let mut buf = [0u8; 4];
    let (bytes, _, unmappable) = encoding_rs::SHIFT_JIS.encode(ch.encode_utf8(&mut buf));
    let cells = if unmappable { tw } else { bytes.len() as i32 };
    // a real double-width glyph runs together; others are placed one at a time, since a
    // proportional font wouldn't land them on the cell grid
    (cells, if tw == 2 && cells == 2 { WIDE } else { SINGLE })
}

fn put_i16(o: &mut Vec<u8>, v: i32) {
    o.extend_from_slice(&(v as i16).to_le_bytes());
}
fn put_i32(o: &mut Vec<u8>, v: i32) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn put_f32(o: &mut Vec<u8>, v: f32) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn put_i64(o: &mut Vec<u8>, v: i64) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn put_str(o: &mut Vec<u8>, s: &str) {
    put_i32(o, s.len() as i32);
    o.extend_from_slice(s.as_bytes());
}

/// A picture's size in the UI: its own, up to a limit.
fn capped(w: u32, h: u32) -> (u32, u32) {
    const CAP: u32 = 1024;
    let big = w.max(h);
    if big > CAP {
        ((w * CAP / big).max(1), (h * CAP / big).max(1))
    } else {
        (w.max(1), h.max(1))
    }
}

/// The columns text wraps to at a text size, in a window that many cells wide.
fn text_cols(window: usize, percent: usize) -> usize {
    (window * 100 / percent.max(1)).max(21)
}

/// The text size for a window that wide: what the player chose, else the game's own unless that
/// would be smaller than the app's minimum, which is `max_cols` columns on this screen.
fn text_percent(window: usize, chosen: Option<usize>, max_cols: usize) -> usize {
    chosen.unwrap_or_else(|| (window * 100).div_ceil(max_cols.max(1)).max(100))
}

struct AndroidFe {
    rx: Receiver<Msg>,
    replay: Option<Replay>,
    /// the columns text wraps to, which the player's text size decides
    width: usize,
    /// the columns the game's window has: scripts and pictures go by this, so that bigger text
    /// doesn't break a menu drawn as a picture
    window: usize,
    text_percent: usize,
    chosen_percent: Option<usize>,
    max_cols: usize,
    /// a tap waiting out its frame: see `input_raw`
    held_tap: Option<RawInput>,
    /// a hotkey chosen while the script ran, kept from `drop_stale` for the next input
    hotkey: Option<Hotkey>,
    printed: usize,
    dropped_seen: usize,
    /// pictures the UI holds: key text -> number, oldest first for eviction
    images: HashMap<String, i32>,
    image_order: VecDeque<(String, i32, usize)>,
    image_bytes: usize,
    image_budget: usize,
    next_image: i32,
    input_done: Instant,
    last_purge: Instant,
}

impl AndroidFe {
    /// Gives freed memory back to Android, at most every few seconds: decoding pictures
    /// leaves a lot of it behind, and on a small phone it can decide whether we get killed.
    fn maybe_purge(&mut self) {
        if self.last_purge.elapsed() >= Duration::from_secs(3) {
            crate::sys::trim_memory();
            self.last_purge = Instant::now();
        }
    }

    /// The screen cell for a position counted up from the bottom row.
    fn cell(&self, col: usize, rows_up: usize) -> (usize, usize) {
        (col, self.rows().saturating_sub(rows_up + 1))
    }

    /// Taps made while the script was running answer nothing. True if the player quit meanwhile.
    fn drop_stale(&mut self, res: &mut Resources) -> bool {
        let mut quit = false;
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Quit => quit = true,
                Msg::Trim => res.release_images(),
                Msg::TextSize(p) => self.set_text_size(p),
                Msg::MaxCols(n) => self.set_max_cols(n),
                Msg::Hotkey(h) => self.hotkey = Some(h),
                _ => {}
            }
        }
        quit
    }

    /// The next update lays out everything again, at the new width.
    fn set_text_size(&mut self, percent: usize) {
        self.chosen_percent = Some(percent);
        self.text_percent = percent;
        self.width = text_cols(self.window, percent);
        self.printed = 0;
    }

    /// A tablet turned: text the player hasn't sized follows the minimum on the new width.
    fn set_max_cols(&mut self, cols: usize) {
        self.max_cols = cols;
        let width = self.width;
        self.set_cols(self.window);
        // (the keyboard opening resizes the view too, and needn't lay anything out again)
        if self.width != width {
            self.printed = 0;
        }
    }

    /// The player's next action, or None at the deadline.
    fn next_msg(&mut self, res: &mut Resources, deadline: Option<Instant>) -> Option<Msg> {
        loop {
            let m = match deadline {
                Some(d) => match self.rx.recv_timeout(d.saturating_duration_since(Instant::now())) {
                    Ok(m) => m,
                    Err(RecvTimeoutError::Timeout) => return None,
                    Err(RecvTimeoutError::Disconnected) => Msg::Quit,
                },
                None => self.rx.recv().unwrap_or(Msg::Quit),
            };
            match m {
                Msg::Drawn { .. } => {}
                Msg::Trim => res.release_images(),
                m => return Some(m),
            }
        }
    }

    /// The picture's number in the UI, sent there the first time. Sent at its natural size
    /// (capped) and scaled by the GPU when drawn: on slow cores CPU scaling cost far more
    /// than decoding in the benchmark.
    fn image_key(&mut self, res: &mut Resources, name: &str, w: f32, h: f32) -> Option<i32> {
        let frame = res.frame_name(name);
        let (nw, nh) = res.sprite_size(&frame).unwrap_or((w.max(1.0) as u32, h.max(1.0) as u32));
        let (tw, th) = capped(nw, nh);
        let id = format!("{}@{}x{}#{}", frame, tw, th, res.version(&frame));
        if let Some(k) = self.images.get(&id) {
            return Some(*k);
        }
        let img = res.scaled(&frame, tw, th)?;
        Some(self.send_image(id, &img))
    }

    /// The client background layers (CBG*), back to front: picture, position and size in
    /// Emuera's pixels, and whether it goes behind the text. Sent with every update: a
    /// layer's picture may have been dropped from the UI to stay within its budget.
    // shortcut: a button sprite always shows its normal picture, as a touch screen has no hover; show its other one under the finger if a game's menu needs that feedback.
    fn cbg_layers(&mut self, res: &mut Resources) -> Vec<(i32, i32, i32, i32, i32, bool)> {
        let mut out = Vec::new();
        for l in res.cbg.clone() {
            let (key, (w, h), (bx, by)) = match &l.src {
                CbgSrc::Sprite(n) => {
                    let frame = res.frame_name(n);
                    let (Some(size), true) = (res.sprite_size(&frame), res.exists(n)) else { continue };
                    let Some(key) = self.image_key(res, n, size.0 as f32, size.1 as f32) else { continue };
                    // (a sprite is offset by its base position, CSV columns 7-8)
                    (key, size, res.base_pos(n))
                }
                CbgSrc::G(_) => {
                    let Some((img, fp)) = res.cbg_pixels(&l.src) else { continue };
                    let (w, h) = img.dimensions();
                    let id = format!("cbg{:x}", fp);
                    let key = match self.images.get(&id) {
                        Some(k) => *k,
                        None if capped(w, h) == (w, h) => self.send_image(id, &img),
                        None => {
                            let (tw, th) = capped(w, h);
                            self.send_image(id, &crate::resources::resize(&img, tw, th))
                        }
                    };
                    (key, (w, h), (0, 0))
                }
            };
            out.push((key, l.x + bx as i32, l.y + by as i32, w as i32, h as i32, l.z > 0));
        }
        out
    }

    /// Gives a picture to the UI, under a new number.
    fn send_image(&mut self, id: String, img: &image::RgbaImage) -> i32 {
        // Android bitmaps hold premultiplied alpha
        let mut px = img.as_raw().clone();
        for p in px.chunks_exact_mut(4) {
            let a = p[3] as u32;
            if a != 255 {
                p[0] = (p[0] as u32 * a / 255) as u8;
                p[1] = (p[1] as u32 * a / 255) as u8;
                p[2] = (p[2] as u32 * a / 255) as u8;
            }
        }
        let key = self.next_image;
        self.next_image += 1;
        let (iw, ih) = img.dimensions();
        let bytes = px.len();
        // A direct buffer over these pixels, used only during the call: no copy lands on
        // the Java heap, where big arrays would make the garbage collector's limits grow.
        with_java(|env, class| {
            // SAFETY: `px` outlives the call, and Kotlin doesn't keep the buffer
            if let Ok(buf) = unsafe { env.new_direct_byte_buffer(px.as_mut_ptr(), px.len()) } {
                let _ = env.call_static_method(
                    class,
                    "onImage",
                    "(IIILjava/nio/ByteBuffer;)V",
                    &[JValue::Int(key), JValue::Int(iw as i32), JValue::Int(ih as i32), JValue::Object(&buf)],
                );
            }
        });
        self.images.insert(id.clone(), key);
        self.image_order.push_back((id, key, bytes));
        self.image_bytes += bytes;
        while self.image_bytes > self.image_budget && self.image_order.len() > 1 {
            let Some((old, k, b)) = self.image_order.pop_front() else { break };
            self.images.remove(&old);
            self.image_bytes -= b;
            with_java(|env, class| {
                let _ = env.call_static_method(class, "onImageDrop", "(I)V", &[JValue::Int(k)]);
            });
        }
        key
    }

    /// `line` as rows of runs, wrapped at the width it was printed at.
    fn layout(&mut self, line: &Line, con: &Console, res: &mut Resources, wrap: i32) -> Vec<Vec<Run>> {
        let cell_px = (con.font_size / 2.0).max(1.0);
        let wrap = wrap.max(20);
        let mut rows: Vec<Vec<Run>> = vec![vec![]];
        let mut col = 0i32;
        for seg in &line.segs {
            match seg {
                Seg::Text { text, style, button, .. } => {
                    let btn = button.as_ref().filter(|v| !is_tip_area(v)).map(|v| v.to_string());
                    let flags = style.bold as u8 | (style.italic as u8) << 1 | (style.underline as u8) << 2 | (style.strike as u8) << 3;
                    let (fg, bg) = (style.fg.to_i64() as i32, style.bg.map_or(-1, |c| c.to_i64() as i32));
                    let mut cur = String::new();
                    let (mut cur_kind, mut cur_col, mut cur_cells) = (NARROW, col, 0);
                    macro_rules! end_run {
                        () => {
                            if !cur.is_empty() {
                                rows.last_mut().unwrap().push(Run {
                                    col: cur_col,
                                    cells: cur_cells,
                                    dy: 0,
                                    kind: cur_kind,
                                    flags,
                                    fg,
                                    bg,
                                    text: std::mem::take(&mut cur),
                                    btn: btn.clone(),
                                    key: 0,
                                    w: 0.0,
                                    h: 0.0,
                                    ypos: 0.0,
                                });
                            }
                        };
                    }
                    for ch in text.chars() {
                        if ch == '\n' {
                            end_run!();
                            rows.push(vec![]);
                            col = 0;
                            cur_cells = 0;
                            cur_col = 0;
                            continue;
                        }
                        let (n, kind) = char_class(ch);
                        if n == 0 {
                            // combining marks ride on the character before them
                            if !cur.is_empty() {
                                cur.push(ch);
                            }
                            continue;
                        }
                        if col + n > wrap {
                            end_run!();
                            rows.push(vec![]);
                            col = 0;
                            cur_cells = 0;
                            cur_col = 0;
                        }
                        if cur.is_empty() || kind != cur_kind || kind == SINGLE {
                            end_run!();
                            cur_kind = kind;
                            cur_col = col;
                            cur_cells = 0;
                        }
                        cur.push(ch);
                        cur_cells += n;
                        col += n;
                    }
                    end_run!();
                }
                Seg::Image { name, w, h, ypos, button, .. } => {
                    let cells = (w / cell_px).ceil().max(1.0) as i32;
                    if let Some(key) = self.image_key(res, name, *w, *h) {
                        let btn = button.as_ref().filter(|v| !is_tip_area(v)).map(|v| v.to_string());
                        rows.last_mut().unwrap().push(Run {
                            col,
                            cells,
                            dy: 0,
                            kind: IMAGE,
                            flags: 0,
                            fg: 0,
                            bg: -1,
                            text: String::new(),
                            btn,
                            key,
                            w: *w,
                            h: *h,
                            ypos: *ypos,
                        });
                    }
                    col += cells;
                }
                Seg::Space { w } => col += (w / cell_px).round() as i32,
                Seg::Rect { w, h, color, button } => {
                    let cells = (w / cell_px).ceil().max(1.0) as i32;
                    let btn = button.as_ref().filter(|v| !is_tip_area(v)).map(|v| v.to_string());
                    rows.last_mut().unwrap().push(Run {
                        col,
                        cells,
                        dy: 0,
                        kind: RECT,
                        flags: 0,
                        fg: color.to_i64() as i32,
                        bg: -1,
                        text: String::new(),
                        btn,
                        key: 0,
                        w: *w,
                        h: *h,
                        ypos: 0.0,
                    });
                    col += cells;
                }
                Seg::Div { dx, dy, cols, lines, deco, .. } => {
                    let ins = deco.insets();
                    let (base_col, base_dy) = (col + dx + ins[3] as i32, *dy + ins[0] as i32);
                    let mut inner: Vec<Vec<Run>> = vec![];
                    for l in lines {
                        inner.extend(self.layout(l, con, res, *cols as i32));
                    }
                    let total = inner.len() as i32 + (ins[0] + ins[2]) as i32;
                    let host = rows.last_mut().unwrap();
                    if let Some(bg) = deco.bg {
                        // no border or rounded corners yet: just its background
                        host.push(Run {
                            col: col + dx,
                            cells: *cols as i32 + (ins[1] + ins[3]) as i32,
                            dy: *dy,
                            kind: BOX,
                            flags: 0,
                            fg: bg.to_i64() as i32,
                            bg: total,
                            text: String::new(),
                            btn: None,
                            key: 0,
                            w: 0.0,
                            h: 0.0,
                            ypos: 0.0,
                        });
                    }
                    for (r, row) in inner.into_iter().enumerate() {
                        for mut run in row {
                            run.col += base_col;
                            run.dy += base_dy + r as i32;
                            host.push(run);
                        }
                    }
                }
            }
        }
        if line.align != Align::Left {
            for row in &mut rows {
                let used = row.iter().filter(|r| r.dy == 0).map(|r| r.col + r.cells).max().unwrap_or(0);
                let shift = match line.align {
                    Align::Center => (wrap - used) / 2,
                    _ => wrap - used,
                }
                .max(0);
                for r in row.iter_mut() {
                    r.col += shift;
                }
            }
        }
        rows
    }

    /// Everything new since the last call goes to the UI (and into the replay hash, with
    /// `Headless::flush`'s logic, so the two count lines the same way).
    fn sync(&mut self, con: &mut Console, res: &mut Resources, kind: u8, want_ack: bool) {
        // (for DRAWLINE and GETLINESTR: a script at INPUTMOUSEKEY may not reach an ordinary input for a long time)
        con.width = self.width.saturating_sub(1).max(20);
        self.printed = self.printed.saturating_sub(con.dropped_front - self.dropped_seen);
        self.dropped_seen = con.dropped_front;
        self.printed = self.printed.min(con.low_water).min(con.lines.len());
        con.low_water = usize::MAX;
        let start = self.printed;
        let end = con.lines.len();
        if let Some(r) = &mut self.replay {
            for l in &con.lines[start..end] {
                r.print(Console::line_text(l).as_bytes());
                r.print(b"\n");
            }
        }
        let mut o = Vec::with_capacity(4096);
        put_i32(&mut o, self.width as i32);
        put_i32(&mut o, self.window as i32);
        put_i32(&mut o, self.text_percent as i32);
        put_i32(&mut o, con.screen_bg.to_i64() as i32);
        put_i32(&mut o, con.focus_fg.to_i64() as i32);
        put_f32(&mut o, con.font_size);
        put_f32(&mut o, con.line_height);
        put_i64(&mut o, con.valid_gen as i64);
        o.push(kind);
        o.push(want_ack as u8);
        put_i32(&mut o, (con.dropped_front + start) as i32);
        put_i32(&mut o, (end - start) as i32);
        for i in start..end {
            let rows = {
                let line = &con.lines[i];
                // (not the width it was printed at: the text size may have changed since)
                self.layout(line, con, res, self.width as i32)
            };
            put_i64(&mut o, con.lines[i].gen as i64);
            put_i32(&mut o, rows.len() as i32);
            for row in &rows {
                put_i32(&mut o, row.len() as i32);
                for r in row {
                    put_i16(&mut o, r.col);
                    put_i16(&mut o, r.cells);
                    put_i16(&mut o, r.dy);
                    o.push(r.kind);
                    o.push(r.flags);
                    put_i32(&mut o, r.fg);
                    put_i32(&mut o, r.bg);
                    match &r.btn {
                        Some(b) => put_str(&mut o, b),
                        None => put_i32(&mut o, -1),
                    }
                    match r.kind {
                        IMAGE => {
                            put_i32(&mut o, r.key);
                            put_f32(&mut o, r.w);
                            put_f32(&mut o, r.h);
                            put_f32(&mut o, r.ypos);
                        }
                        RECT => {
                            put_f32(&mut o, r.w);
                            put_f32(&mut o, r.h);
                        }
                        BOX => {}
                        _ => put_str(&mut o, &r.text),
                    }
                }
            }
        }
        let layers = self.cbg_layers(res);
        put_i32(&mut o, layers.len() as i32);
        for (key, x, y, w, h, behind) in layers {
            put_i32(&mut o, key);
            put_i32(&mut o, x);
            put_i32(&mut o, y);
            put_i32(&mut o, w);
            put_i32(&mut o, h);
            o.push(behind as u8);
        }
        self.printed = end;
        con.dirty = false;
        with_java(|env, class| {
            if let Ok(arr) = env.byte_array_from_slice(&o) {
                let _ = env.call_static_method(class, "onUpdate", "([B)V", &[JValue::Object(&arr)]);
            }
        });
    }

    fn input_replay(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq) -> InputResult {
        let t_enter = Instant::now();
        let elapsed = self.input_done.elapsed().as_micros() as u64;
        // `Headless::input`, step for step: what it prints is what gets hashed
        let partial = (!con.cur.is_empty()).then(|| Line {
            segs: con.cur.clone(),
            align: con.align,
            gen: con.gen,
            temporary: false,
            cols: con.width + 1,
            kojo: false,
            src: 0,
            wait: con.waits,
        });
        let is_wait = req.kind == InputKind::Wait;
        self.sync(con, res, 0, !is_wait);
        let r = self.replay.as_mut().unwrap();
        if let Some(p) = &partial {
            r.print(Console::line_text(p).as_bytes());
        }
        if is_wait {
            r.print(b"\n");
            r.prep_us += t_enter.elapsed().as_micros() as u64;
            return InputResult::Text(String::new());
        }
        let vals: Vec<String> = con.button_values().iter().map(|v| v.to_string().replace(',', "\\,")).collect();
        if !vals.is_empty() {
            r.print(format!("[buttons: {}]\n", vals.join(", ")).as_bytes());
        }
        self.maybe_purge();
        let r = self.replay.as_mut().unwrap();
        // from here on, as at erbbench's marker: the step is over
        let prep = r.prep_us + t_enter.elapsed().as_micros() as u64;
        let vm_us = elapsed.saturating_sub(r.prep_us);
        let step = r.vm.len();
        if step == 0 {
            r.first_input_ms = r.started.elapsed().as_millis();
        }
        let t_ack = Instant::now();
        let (mut parse_us, mut draw_us) = (0, 0);
        loop {
            match self.rx.recv_timeout(Duration::from_secs(10)) {
                Ok(Msg::Drawn { parse_us: p, draw_us: d }) => {
                    parse_us = p;
                    draw_us = d;
                    break;
                }
                Ok(Msg::Quit) => {
                    let r = self.replay.as_mut().unwrap();
                    r.answers.clear();
                    break;
                }
                Ok(_) => continue,
                Err(_) => {
                    self.replay.as_mut().unwrap().late_frames += 1;
                    break;
                }
            }
        }
        let ack_us = t_ack.elapsed().as_micros() as u64;
        let rss = proc_status("VmRSS:");
        let r = self.replay.as_mut().unwrap();
        r.csv += &format!("{step},{vm_us},{prep},{ack_us},{parse_us},{draw_us},{rss}\n");
        r.hashes += &format!("{:016x}\n", r.step_hash);
        r.step_hash = HASH_START;
        r.vm.push(vm_us);
        r.prep_us = 0;
        r.prep_total += prep;
        r.ack_total += ack_us;
        r.parse_total += parse_us;
        r.draw_total += draw_us;
        match r.answers.pop_front() {
            Some(a) => {
                r.print(b"\n");
                self.input_done = Instant::now();
                InputResult::Line(a)
            }
            None => InputResult::Quit,
        }
    }

    /// Writes the replay's results. The engine may still print on its way out, so this
    /// runs at shutdown, and the output hash covers all of it.
    fn finish_replay(&mut self) {
        let Some(r) = &mut self.replay else { return };
        let _ = std::fs::create_dir_all(&r.dir);
        let _ = std::fs::write(r.dir.join("steps.csv"), format!("step,vm_us,prep_us,ack_us,parse_us,draw_us,rss_kb\n{}", r.csv));
        let _ = std::fs::write(r.dir.join("hashes.txt"), &r.hashes);
        let mut lat: Vec<u64> = r.vm.iter().skip(1).copied().collect();
        lat.sort_unstable();
        let pct = |p: f64| lat.get(((lat.len().max(1) - 1) as f64 * p).round() as usize).copied().unwrap_or(0) as f64 / 1000.0;
        let s = format!(
            "steps={} first_input_ms={} p50_ms={:.2} p90_ms={:.2} p99_ms={:.1} max_ms={:.0} over100ms={} over500ms={} over2s={} vm_total_s={:.2} prep_total_s={:.2} ack_total_s={:.2} parse_total_s={:.2} draw_total_s={:.2} late_frames={} wall_s={:.1} peak_rss_mb={} output_hash={:016x}\n",
            lat.len(),
            r.first_input_ms,
            pct(0.5),
            pct(0.9),
            pct(0.99),
            pct(1.0),
            lat.iter().filter(|&&u| u > 100_000).count(),
            lat.iter().filter(|&&u| u > 500_000).count(),
            lat.iter().filter(|&&u| u > 2_000_000).count(),
            lat.iter().sum::<u64>() as f64 / 1e6,
            r.prep_total as f64 / 1e6,
            r.ack_total as f64 / 1e6,
            r.parse_total as f64 / 1e6,
            r.draw_total as f64 / 1e6,
            r.late_frames,
            r.started.elapsed().as_secs_f64(),
            proc_status("VmHWM:") / 1024,
            r.hash,
        );
        let _ = std::fs::write(r.dir.join("summary.txt"), s);
    }
}

impl Frontend for AndroidFe {
    fn set_game(&mut self, game: &std::path::Path, save_dir: &std::path::Path) {
        *GAME.lock().unwrap() = Some((game.to_path_buf(), save_dir.to_path_buf()));
    }
    fn flush(&mut self, con: &mut Console, res: &mut Resources) {
        let t = Instant::now();
        self.sync(con, res, 0, false);
        if let Some(r) = &mut self.replay {
            r.prep_us += t.elapsed().as_micros() as u64;
        }
    }

    fn input(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq) -> InputResult {
        if self.replay.is_some() {
            return self.input_replay(con, res, req);
        }
        self.held_tap = None;
        let kind = match req.kind {
            InputKind::Int => 1,
            InputKind::Str => 2,
            InputKind::Wait => 3,
        };
        self.sync(con, res, kind, false);
        self.maybe_purge();
        let width = self.width;
        if self.drop_stale(res) {
            return InputResult::Quit;
        }
        if let Some(h) = self.hotkey.take() {
            return InputResult::Hotkey(h);
        }
        if self.width != width {
            self.sync(con, res, kind, false);
        }
        let deadline = req.timeout_ms.map(|ms| Instant::now() + Duration::from_millis(ms));
        loop {
            return match self.next_msg(res, deadline) {
                None => InputResult::Timeout,
                Some(Msg::TextSize(p)) => {
                    self.set_text_size(p);
                    self.sync(con, res, kind, false);
                    continue;
                }
                Some(Msg::MaxCols(n)) => {
                    self.set_max_cols(n);
                    self.sync(con, res, kind, false);
                    continue;
                }
                Some(Msg::Click(v)) => InputResult::Text(v),
                Some(Msg::Line(s)) => InputResult::Line(s),
                Some(Msg::Skip) => InputResult::Skip(String::new()),
                Some(Msg::Hotkey(h)) => InputResult::Hotkey(h),
                Some(Msg::Quit) => InputResult::Quit,
                // a long press, or left over from an INPUTMOUSEKEY
                Some(_) => continue,
            };
        }
    }

    /// INPUTMOUSEKEY: every tap goes to the script, wherever it lands.
    fn input_raw(&mut self, con: &mut Console, res: &mut Resources, timeout_ms: Option<u64>) -> RawInput {
        if self.replay.is_some() {
            let req = InputReq { kind: InputKind::Str, one: false, timeout_ms, stop_skip: false };
            return raw_from_input(self.input(con, res, &req));
        }
        self.sync(con, res, RAW, false);
        self.maybe_purge();
        if let Some(click) = self.held_tap.take() {
            return click;
        }
        let width = self.width;
        // (a timed one is a real-time game's frame: a tap between two frames still counts)
        if timeout_ms.is_none() && self.drop_stale(res) {
            return RawInput::Quit;
        }
        if let Some(h) = self.hotkey.take() {
            return RawInput::Hotkey(h);
        }
        if self.width != width {
            self.sync(con, res, RAW, false);
        }
        let deadline = timeout_ms.map(|ms| Instant::now() + Duration::from_millis(ms));
        loop {
            return match self.next_msg(res, deadline) {
                None => RawInput::Timeout,
                Some(Msg::TextSize(p)) => {
                    self.set_text_size(p);
                    self.sync(con, res, RAW, false);
                    continue;
                }
                Some(Msg::MaxCols(n)) => {
                    self.set_max_cols(n);
                    self.sync(con, res, RAW, false);
                    continue;
                }
                Some(Msg::Tap { btn, col, rows_up, button }) => {
                    let button = button.map(|v| v.parse().map_or(Value::Str(v), Value::Int));
                    let click = RawInput::Click { btn, cell: self.cell(col, rows_up), button };
                    // A mouse is over something before it clicks it, and scripts work out what's
                    // under it once a frame (pops-tw's title), but a finger lands and lifts within
                    // one: a frame goes by first, or the click acts on where the finger was before.
                    if timeout_ms.is_some() {
                        self.held_tap = Some(click);
                        RawInput::Timeout
                    } else {
                        click
                    }
                }
                Some(Msg::Wheel { up, col, rows_up }) => RawInput::Wheel { up, cell: self.cell(col, rows_up) },
                Some(Msg::Line(s)) => raw_from_input(InputResult::Line(s)),
                Some(Msg::Skip) => raw_from_input(InputResult::Skip(String::new())),
                Some(Msg::Hotkey(h)) => RawInput::Hotkey(h),
                Some(Msg::Quit) => RawInput::Quit,
                Some(_) => continue,
            };
        }
    }

    fn mouse_cell(&self) -> Option<(usize, usize)> {
        let p = POINTER.lock().unwrap();
        p.touched.then(|| self.cell(p.col, p.rows_up))
    }

    fn hover_value(&self) -> String {
        POINTER.lock().unwrap().button.clone()
    }

    fn key_state(&mut self, code: i64, triggered: bool) -> bool {
        let mut p = POINTER.lock().unwrap();
        match (code, triggered) {
            (1, false) => p.down,
            (1, true) => std::mem::take(&mut p.left_hit),
            // a long press is a moment, not a button held: it counts until it's read
            (2, false) => p.right_hit,
            (2, true) => std::mem::take(&mut p.right_hit),
            _ => false,
        }
    }

    fn cols(&self) -> usize {
        self.width
    }
    fn set_cols(&mut self, cols: usize) {
        self.window = cols;
        self.text_percent = text_percent(cols, self.chosen_percent, self.max_cols);
        self.width = text_cols(cols, self.text_percent);
    }
    fn window_cols(&self) -> Option<usize> {
        self.replay.is_none().then_some(self.window)
    }
    // Replay answers as `Headless` does wherever that could change what a script does.
    fn rows(&self) -> usize {
        if self.replay.is_some() {
            40
        } else {
            VIEW_ROWS.load(Ordering::Relaxed)
        }
    }
    fn is_fullscreen(&self) -> bool {
        self.replay.is_none()
    }
    fn live_updates(&self) -> bool {
        self.replay.is_none()
    }
    fn poll_interrupt(&mut self) -> bool {
        QUIT.load(Ordering::Relaxed)
    }
    fn shutdown(&mut self) {
        self.finish_replay();
    }
}

fn get_string(env: &mut JNIEnv, s: &JString) -> Option<String> {
    if s.is_null() {
        return None;
    }
    env.get_string(s).ok().map(Into::into)
}

/// `AFFINITY=0f` pins the whole process, UI and render threads included, to those cores:
/// a stand-in for a phone that only has slow ones. New threads inherit it.
fn pin_process(mask: u64) {
    let Ok(rd) = std::fs::read_dir("/proc/self/task") else { return };
    for e in rd.flatten() {
        let Some(tid) = e.file_name().to_str().and_then(|s| s.parse::<i32>().ok()) else { continue };
        unsafe {
            let mut set: libc::cpu_set_t = std::mem::zeroed();
            for c in 0..64 {
                if mask >> c & 1 == 1 {
                    libc::CPU_SET(c, &mut set);
                }
            }
            libc::sched_setaffinity(tid, std::mem::size_of::<libc::cpu_set_t>(), &set);
        }
    }
}

/// Drops the game's files from the page cache, so a load reads the storage as it would
/// after a reboot (file metadata stays cached, so a true first start can be slower).
fn evict(dir: &std::path::Path) {
    use std::os::fd::AsRawFd;
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            evict(&p);
        } else if let Ok(f) = std::fs::File::open(&p) {
            unsafe {
                libc::posix_fadvise(f.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
            }
        }
    }
}

/// Starts the VM. `env_list`: extra settings, as `KEY=VALUE` lines. Besides the engine's
/// own: `AFFINITY=hexmask`, `EVICT=1` (see `evict`), `STEPS=n` (end a replay early), `PANIC=1`
/// (panic at the start, to test the crash report).
#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_start<'a>(
    mut env: JNIEnv<'a>,
    class: JClass<'a>,
    game: JString<'a>,
    data: JString<'a>,
    replay: JString<'a>,
    env_list: JString<'a>,
    cols: jint,
) {
    let (Some(game), Some(data)) = (get_string(&mut env, &game), get_string(&mut env, &data)) else { return };
    let replay = get_string(&mut env, &replay);
    let env_list = get_string(&mut env, &env_list).unwrap_or_default();
    let (Ok(vm), Ok(class)) = (env.get_java_vm(), env.new_global_ref(&class)) else { return };
    let (tx, rx) = channel();
    if SHARED.set(Shared { vm, class, tx }).is_err() {
        return;
    }
    let data = PathBuf::from(data);
    // the engine's stderr, with its timing report and any panic, goes to a file
    // (the last run's is kept: it may be what someone needs to report a crash)
    let _ = std::fs::rename(data.join("engine.log"), data.join("engine.log.1"));
    if let Ok(f) = std::fs::File::create(data.join("engine.log")) {
        use std::os::fd::IntoRawFd;
        let fd = f.into_raw_fd();
        unsafe {
            libc::dup2(fd, 1);
            libc::dup2(fd, 2);
        }
    }
    // SAFETY: nothing else is running in this library yet
    unsafe {
        std::env::set_var("HOME", &data);
        std::env::set_var("XDG_CONFIG_HOME", data.join("cfg"));
        std::env::set_var("XDG_DATA_HOME", data.join("data"));
        std::env::set_var("EMUERA_TIMING", "1");
        for kv in env_list.lines() {
            if let Some((k, v)) = kv.split_once('=') {
                match k {
                    "AFFINITY" => pin_process(u64::from_str_radix(v, 16).unwrap_or(u64::MAX)),
                    "EVICT" => {
                        libc::sync();
                        evict(std::path::Path::new(&game));
                    }
                    _ => std::env::set_var(k, v),
                }
            }
        }
        // Phone-sized limits, unless told otherwise. The engine's defaults scale with RAM
        // and rarely release anything on a phone, while tight ones cost no measurable speed.
        let small = crate::sys::total_memory().is_some_and(|m| m <= 3 << 30);
        for (k, tight, roomy) in [
            ("EMUERA_CODE_MB", "24", "48"),
            ("EMUERA_GBUF_MB", "32", "64"),
            ("EMUERA_PICTURES_MB", "32", "64"),
            ("EMUERA_CUTS_MB", "16", "32"),
            ("EMUERA_APP_IMAGE_MB", "16", "32"),
        ] {
            if std::env::var_os(k).is_none() {
                std::env::set_var(k, if small { tight } else { roomy });
            }
        }
    }
    let replay = replay.map(|file| {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let mut answers: VecDeque<String> = text.lines().map(str::to_string).collect();
        if let Some(n) = std::env::var("STEPS").ok().and_then(|v| v.parse::<usize>().ok()) {
            answers.truncate(n);
        }
        Replay {
            answers,
            hash: HASH_START,
            step_hash: HASH_START,
            hashes: String::new(),
            csv: String::new(),
            vm: vec![],
            started: Instant::now(),
            first_input_ms: 0,
            prep_us: 0,
            prep_total: 0,
            ack_total: 0,
            parse_total: 0,
            draw_total: 0,
            late_frames: 0,
            dir: data.join("replay"),
        }
    });
    let headless = replay.is_some();
    let size = 64usize << 20;
    let spawned = std::thread::Builder::new().stack_size(size).spawn(move || {
        crate::vm::set_stack_bytes(size);
        crate::i18n::select(crate::settings::Settings::load().get("language"));
        let window = cols.max(20) as usize;
        let chosen_percent = std::env::var("TEXT_PERCENT").ok().and_then(|v| v.parse().ok());
        let max_cols = std::env::var("MAX_TEXT_COLS").ok().and_then(|v| v.parse().ok()).unwrap_or(usize::MAX);
        let percent = text_percent(window, chosen_percent, max_cols);
        let fe = AndroidFe {
            rx,
            replay,
            width: text_cols(window, percent),
            window,
            text_percent: percent,
            chosen_percent,
            max_cols,
            held_tap: None,
            hotkey: None,
            printed: 0,
            dropped_seen: 0,
            images: HashMap::new(),
            image_order: VecDeque::new(),
            image_bytes: 0,
            image_budget: std::env::var("EMUERA_APP_IMAGE_MB").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(96) << 20,
            next_image: 1,
            input_done: Instant::now(),
            last_purge: Instant::now(),
        };
        *crate::frontend::EXTERNAL.lock().unwrap() = Some(Box::new(fe));
        let mut o = crate::default_opts();
        o.game = Some(PathBuf::from(game));
        // the same paths as --headless, which the reference session was recorded with
        o.headless = headless;
        // A panic's message is in the log by now. Without this the thread would just end, with
        // the UI left waiting on a frozen screen.
        let code = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assert!(std::env::var_os("PANIC").is_none(), "PANIC is set");
            crate::run(o)
        }))
        .unwrap_or(101);
        with_java(|env, class| {
            let _ = env.call_static_method(class, "onExit", "(I)V", &[JValue::Int(code)]);
        });
    });
    if let Err(e) = spawned {
        eprintln!("cannot start the VM thread: {}", e);
    }
}

/// Runs `f`, a panic becoming an error: unwinding into Java would abort the app with no report.
fn guarded<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|p| {
        let msg = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned());
        Err(format!("internal error: {}", msg.unwrap_or_default()))
    })
}

/// The error text for Kotlin, "" for success.
fn result_string(env: &mut JNIEnv, r: Result<(), String>) -> jni::sys::jstring {
    env.new_string(r.err().unwrap_or_default()).map_or(std::ptr::null_mut(), |s| s.into_raw())
}

/// Unpacks an archive the system gave us as a file descriptor into `tmp`. Called on a
/// worker thread, and it may be a process that has no game running.
#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_importArchive<'a>(
    mut env: JNIEnv<'a>,
    class: JClass<'a>,
    fd: jint,
    suffix: JString<'a>,
    tmp: JString<'a>,
) -> jni::sys::jstring {
    use std::os::fd::FromRawFd;
    let (Some(suffix), Some(tmp)) = (get_string(&mut env, &suffix), get_string(&mut env, &tmp)) else {
        return result_string(&mut env, Err("bad arguments".into()));
    };
    let r = guarded(|| {
        // our own descriptor, which `File` closes: Kotlin keeps and closes the original
        let own = unsafe { libc::dup(fd) };
        if own < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let f = unsafe { std::fs::File::from_raw_fd(own) };
        std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
        crate::archive::unpack_file(f, &suffix, std::path::Path::new(&tmp), &mut |n| {
            let _ = env.call_static_method(&class, "onImportProgress", "(I)V", &[JValue::Int(n as i32)]);
        })
    });
    result_string(&mut env, r)
}

#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_finishImport<'a>(
    mut env: JNIEnv<'a>,
    _c: JClass<'a>,
    tmp: JString<'a>,
    games: JString<'a>,
    name: JString<'a>,
) -> jni::sys::jstring {
    let (Some(tmp), Some(games), Some(name)) = (get_string(&mut env, &tmp), get_string(&mut env, &games), get_string(&mut env, &name)) else {
        return result_string(&mut env, Err("bad arguments".into()));
    };
    let r = guarded(|| crate::archive::finish_import(std::path::Path::new(&tmp), std::path::Path::new(&games), &name));
    result_string(&mut env, r)
}

#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_recoverImports<'a>(mut env: JNIEnv<'a>, _c: JClass<'a>, games: JString<'a>) {
    let Some(games) = get_string(&mut env, &games) else { return };
    let _ = guarded(|| {
        crate::archive::recover_imports(std::path::Path::new(&games));
        Ok(())
    });
}

/// The settings of emuera-tty's game settings page that do something here: not the clipboard's or the keyboard
/// macros' (a phone has neither), button wrapping (this frontend's layout doesn't follow it), or the font for text
/// drawn into pictures (found with fc-match, which Android doesn't have).
fn android_setting(it: &crate::emuconfig::Item) -> bool {
    it.tab != "Clipboard" && !matches!(it.jp, "キーボードマクロを使用する" | "ボタンの途中で行を折りかえさない" | "フォント名")
}

/// The game's settings for the app's settings dialog, a line each: key, label, tab, kind (bool, int:LOW:HIGH, color),
/// the value in effect, and where it's from (emuera, game, player, or fixed: the game doesn't allow changing it).
/// Called in either process, with or without a game running.
#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_gameSettings<'a>(
    mut env: JNIEnv<'a>,
    _c: JClass<'a>,
    game: JString<'a>,
) -> jni::sys::jstring {
    use crate::emuconfig::{normalize, Kind, Source, Values, ITEMS};
    let Some(game) = get_string(&mut env, &game) else { return std::ptr::null_mut() };
    // (after a panic: no settings)
    let out = guarded(|| {
        let vals = Values::read(std::path::Path::new(&game));
        let mut out = String::new();
        for it in ITEMS.iter().filter(|it| android_setting(it)) {
            let (v, src) = vals.get(it);
            // (a value Emuera can't read is its default there too)
            let v = normalize(it, &v).unwrap_or_else(|_| it.default.to_string());
            let kind = match it.kind {
                Kind::Bool => "bool".to_string(),
                Kind::Int(lo, hi) => format!("int:{lo}:{hi}"),
                Kind::Color => "color".into(),
                Kind::Text => "text".into(),
            };
            let src = match src {
                Source::Emuera => "emuera",
                Source::Game => "game",
                Source::Player => "player",
                Source::Fixed => "fixed",
            };
            out += &format!("{}\t{}\t{}\t{}\t{}\t{}\n", it.jp, it.label, it.tab, kind, v.replace(['\t', '\n'], " "), src);
        }
        Ok(out)
    })
    .unwrap_or_default();
    env.new_string(out).map_or(std::ptr::null_mut(), |s| s.into_raw())
}

/// Sets one of those settings in the game's emuera.config, checked as the settings page checks it; a null value
/// takes the player's out, back to the game's or Emuera's.
#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_setGameSetting<'a>(
    mut env: JNIEnv<'a>,
    _c: JClass<'a>,
    game: JString<'a>,
    key: JString<'a>,
    value: JString<'a>,
) -> jni::sys::jstring {
    use crate::emuconfig::{normalize, Values, ITEMS};
    let (Some(game), Some(key)) = (get_string(&mut env, &game), get_string(&mut env, &key)) else {
        return result_string(&mut env, Err("bad arguments".into()));
    };
    let value = get_string(&mut env, &value);
    let r = guarded(|| {
        let it = ITEMS.iter().find(|it| it.jp == key && android_setting(it)).ok_or_else(|| format!("no setting {key}"))?;
        let value = value.map(|v| normalize(it, &v)).transpose()?;
        Values::read(std::path::Path::new(&game)).set(it, value.as_deref())
    });
    result_string(&mut env, r)
}

fn send(m: Msg) {
    if let Some(sh) = SHARED.get() {
        let _ = sh.tx.send(m);
    }
}

/// The game's menu: back to the title (as emuera-tty's Ctrl-T, unsaved progress is lost).
#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_toTitle<'a>(_env: JNIEnv<'a>, _c: JClass<'a>) {
    send(Msg::Hotkey(Hotkey::Title));
}

/// The game's settings were changed in its menu: the running game reads them again at its next input.
#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_reloadConfig<'a>(_env: JNIEnv<'a>, _c: JClass<'a>) {
    send(Msg::Hotkey(Hotkey::Config));
}

/// The game's menu: read the ERB files again, keeping the game's state (emuera-tty's F5).
#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_reloadErb<'a>(_env: JNIEnv<'a>, _c: JClass<'a>) {
    send(Msg::Hotkey(Hotkey::Reload));
}

// --- the save manager, as emuera-tty's (`crate::savemgr`), for the running game: these run on the UI thread

/// The running game's save folder and where its backups go.
fn save_dirs() -> Result<(PathBuf, PathBuf), String> {
    let (game, dir) = GAME.lock().unwrap().clone().ok_or("the game hasn't started yet")?;
    let root = crate::savemgr::backup_root(&game).unwrap_or_else(|| dir.join("backups"));
    Ok((dir, root))
}

/// For Kotlin: data, or "" when there is none; an error's text after a \u{1}.
fn data_string(env: &mut JNIEnv, r: Result<String, String>) -> jni::sys::jstring {
    env.new_string(r.unwrap_or_else(|e| format!("\u{1}{e}"))).map_or(std::ptr::null_mut(), |s| s.into_raw())
}

/// The saves, a line each: slot ("" for global.sav), path, when it was saved, its comment.
#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_saveList<'a>(mut env: JNIEnv<'a>, _c: JClass<'a>) -> jni::sys::jstring {
    let r = guarded(|| {
        save_dirs().map(|(dir, _)| {
            let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
            let mut out = String::new();
            for s in crate::savemgr::list(&dir) {
                let slot = s.slot.map_or(String::new(), |n| n.to_string());
                out += &format!("{slot}\t{}\t{}\t{}\n", s.path.display(), crate::savemgr::when(s.modified), clean(&s.comment));
            }
            out
        })
    });
    data_string(&mut env, r)
}

/// Copies (or moves) a save into a slot; one there goes to the backups first.
#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_saveCopy<'a>(
    mut env: JNIEnv<'a>,
    _c: JClass<'a>,
    path: JString<'a>,
    slot: jint,
    moving: jboolean,
) -> jni::sys::jstring {
    let Some(path) = get_string(&mut env, &path) else { return result_string(&mut env, Err("bad arguments".into())) };
    let r = guarded(|| {
        save_dirs().and_then(|(dir, root)| {
            let slot = u32::try_from(slot).map_err(|_| "no such slot".to_string())?;
            crate::savemgr::copy_to_slot(std::path::Path::new(&path), &dir, slot, &root, moving != 0).map(|_| ())
        })
    });
    result_string(&mut env, r)
}

/// Deletes a save, after copying it to the backups.
#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_saveDelete<'a>(
    mut env: JNIEnv<'a>,
    _c: JClass<'a>,
    path: JString<'a>,
) -> jni::sys::jstring {
    let Some(path) = get_string(&mut env, &path) else { return result_string(&mut env, Err("bad arguments".into())) };
    let r = guarded(|| save_dirs().and_then(|(_, root)| crate::savemgr::delete(std::path::Path::new(&path), &root)));
    result_string(&mut env, r)
}

/// Imports a save file into a slot, or as global.sav with a negative one. Refuses what isn't an Emuera save.
#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_saveImport<'a>(
    mut env: JNIEnv<'a>,
    _c: JClass<'a>,
    file: JString<'a>,
    slot: jint,
) -> jni::sys::jstring {
    let Some(file) = get_string(&mut env, &file) else { return result_string(&mut env, Err("bad arguments".into())) };
    let r =
        guarded(|| save_dirs().and_then(|(dir, root)| crate::savemgr::import(std::path::Path::new(&file), &dir, u32::try_from(slot).ok(), &root).map(|_| ())));
    result_string(&mut env, r)
}

/// Copies every save into a new backup folder: how many.
#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_saveBackup<'a>(mut env: JNIEnv<'a>, _c: JClass<'a>) -> jni::sys::jstring {
    let r = guarded(|| save_dirs().and_then(|(dir, root)| crate::savemgr::backup_all(&dir, &root, "").map(|(_, n)| n.to_string())));
    data_string(&mut env, r)
}

/// The backups, newest first as the save manager lists them, a line each: folder, how many files.
#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_saveBackups<'a>(mut env: JNIEnv<'a>, _c: JClass<'a>) -> jni::sys::jstring {
    let r = guarded(|| save_dirs().map(|(_, root)| crate::savemgr::backups(&root).iter().map(|(p, n)| format!("{}\t{}\n", p.display(), n)).collect()));
    data_string(&mut env, r)
}

/// Replaces the saves with a backup's, the current ones backed up first: how many were restored.
#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_saveRestore<'a>(
    mut env: JNIEnv<'a>,
    _c: JClass<'a>,
    from: JString<'a>,
) -> jni::sys::jstring {
    let Some(from) = get_string(&mut env, &from) else { return data_string(&mut env, Err("bad arguments".into())) };
    let r = guarded(|| save_dirs().and_then(|(dir, root)| crate::savemgr::restore(std::path::Path::new(&from), &dir, &root).map(|n| n.to_string())));
    data_string(&mut env, r)
}

#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_click<'a>(mut env: JNIEnv<'a>, _c: JClass<'a>, v: JString<'a>) {
    if let Some(v) = get_string(&mut env, &v) {
        send(Msg::Click(v));
    }
}

#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_line<'a>(mut env: JNIEnv<'a>, _c: JClass<'a>, v: JString<'a>) {
    if let Some(v) = get_string(&mut env, &v) {
        send(Msg::Line(v));
    }
}

#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_skip<'a>(_env: JNIEnv<'a>, _c: JClass<'a>) {
    send(Msg::Skip);
}

#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_quit<'a>(_env: JNIEnv<'a>, _c: JClass<'a>) {
    QUIT.store(true, Ordering::Relaxed);
    send(Msg::Quit);
}

#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_pointer<'a>(
    mut env: JNIEnv<'a>,
    _c: JClass<'a>,
    down: jboolean,
    col: jint,
    rows_up: jint,
    button: JString<'a>,
) {
    let button = get_string(&mut env, &button).unwrap_or_default();
    let down = down != 0;
    let mut p = POINTER.lock().unwrap();
    p.left_hit |= down && !p.down;
    p.touched = true;
    p.down = down;
    p.col = col as usize;
    p.rows_up = rows_up as usize;
    p.button = button;
}

#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_tap<'a>(
    mut env: JNIEnv<'a>,
    _c: JClass<'a>,
    btn: jint,
    col: jint,
    rows_up: jint,
    button: JString<'a>,
) {
    send(Msg::Tap { btn: btn as u8, col: col as usize, rows_up: rows_up as usize, button: get_string(&mut env, &button) });
}

#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_rightPress<'a>(_env: JNIEnv<'a>, _c: JClass<'a>) {
    POINTER.lock().unwrap().right_hit = true;
}

#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_wheel<'a>(_env: JNIEnv<'a>, _c: JClass<'a>, up: jboolean, col: jint, rows_up: jint) {
    send(Msg::Wheel { up: up != 0, col: col as usize, rows_up: rows_up as usize });
}

#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_textSize<'a>(_env: JNIEnv<'a>, _c: JClass<'a>, percent: jint) {
    send(Msg::TextSize(percent.max(1) as usize));
}

#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_maxTextCols<'a>(_env: JNIEnv<'a>, _c: JClass<'a>, cols: jint) {
    send(Msg::MaxCols(cols.max(1) as usize));
}

#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_viewRows<'a>(_env: JNIEnv<'a>, _c: JClass<'a>, rows: jint) {
    VIEW_ROWS.store(rows.max(1) as usize, Ordering::Relaxed);
}

#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_trim<'a>(_env: JNIEnv<'a>, _c: JClass<'a>) {
    send(Msg::Trim);
}

#[no_mangle]
pub extern "system" fn Java_io_github_astartes_1gaming_emuera_Native_drawn<'a>(_env: JNIEnv<'a>, _c: JClass<'a>, parse_us: jlong, draw_us: jlong) {
    send(Msg::Drawn { parse_us, draw_us });
}
