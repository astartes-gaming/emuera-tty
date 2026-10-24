//! kitty graphics protocol backend (also works in WezTerm, Ghostty, Konsole).
//!
//! Each distinct scaled image is transmitted once (`a=t`, RGBA, base64 in
//! 4 KiB chunks) under a numeric id. Frames only touch placements that
//! changed: an image whose placements are the same as last frame is left
//! alone (deleting and re-placing everything made pictures flash while a
//! script printed text a character at a time); otherwise its placements
//! are deleted (`a=d,d=i`, keeping the data) and placed again (`a=p`).
//! `C=1` keeps the text cursor still; `y`/`h` crop, `c`/`r` give the size
//! in cells, `Y` offsets inside the cell.
//!
//! Inside tmux (`Kitty::new_tmux`), every command is wrapped in tmux's
//! passthrough (`ESC P tmux; …`, needs `allow-passthrough on`), and
//! pictures use kitty's Unicode placeholders: each image gets one virtual
//! placement (`U=1`, its size in cells), and its cells are written as text,
//! U+10EEEE with the image id as the 256-colour foreground and two
//! diacritics for the cell's row and column. tmux keeps that text where it
//! belongs (panes, windows, scrolling, redraws) and the terminal draws the
//! picture there. A placeholder is a cell's text, so a layer meant to sit
//! behind the text is left out, and crops are to whole cells.

use super::{Backend, Placement};
use base64::Engine;
use std::collections::HashMap;
use std::io::Write;

/// Bytes of off-screen pictures kept in the terminal (see `Kitty::draw`).
const PARKED_BUDGET: usize = 24 << 20;

pub struct Kitty {
    /// placement key (sprite + size) → image id already in the terminal
    ids: HashMap<String, u32>,
    next_id: u32,
    /// image id → its size in the terminal (RGBA bytes)
    size: HashMap<u32, usize>,
    /// pictures off screen but kept in the terminal (to be shown again
    /// without sending them), least recently shown first, and their bytes
    parked: std::collections::VecDeque<u32>,
    parked_bytes: usize,
    /// ids of freed pictures, given to the next new ones: Konsole never
    /// frees a picture's data (it reads `d=I` as `d=i`, 25.04), but
    /// replaces it when another comes with the same id, so new ids for
    /// every frame of a canvas game would pile up there without end
    free_ids: Vec<u32>,
    /// what is on screen: image id → its placement commands
    shown: HashMap<u32, Vec<String>>,
    /// inside tmux: passthrough and Unicode placeholders
    tmux: bool,
    /// tmux: image id → the size in cells of its virtual placement
    virt: HashMap<u32, (u32, u32)>,
    /// tmux: pictures moved down inside their first cell (`y_off`), as
    /// copies with that many transparent rows on top, by key
    padded: HashMap<String, std::rc::Rc<image::RgbaImage>>,
    /// tmux: image id → which of its cells (row by row) show anything; a
    /// fully transparent cell gets no placeholder, so text there stays
    opaque: HashMap<u32, Vec<bool>>,
}

/// Which cells of `img`, cut into `cols` × `rows` cells of `cw` × `ch`
/// pixels, hold a pixel that isn't fully transparent (row by row).
fn opaque_cells(img: &image::RgbaImage, cols: u32, rows: u32, cw: u32, ch: u32) -> Vec<bool> {
    let mut m = vec![false; (cols * rows) as usize];
    for (x, y, px) in img.enumerate_pixels() {
        if px.0[3] != 0 {
            let (c, r) = (x / cw, y / ch);
            if c < cols && r < rows {
                m[(r * cols + c) as usize] = true;
            }
        }
    }
    m
}

/// kitty's row / column diacritics for Unicode placeholders
/// (rowcolumn-diacritics.txt), in order: the n-th means row or column n.
const DIACRITICS: [char; 297] = [
    '\u{0305}', '\u{030D}', '\u{030E}', '\u{0310}', '\u{0312}', '\u{033D}', '\u{033E}', '\u{033F}', '\u{0346}', '\u{034A}', '\u{034B}', '\u{034C}', '\u{0350}',
    '\u{0351}', '\u{0352}', '\u{0357}', '\u{035B}', '\u{0363}', '\u{0364}', '\u{0365}', '\u{0366}', '\u{0367}', '\u{0368}', '\u{0369}', '\u{036A}', '\u{036B}',
    '\u{036C}', '\u{036D}', '\u{036E}', '\u{036F}', '\u{0483}', '\u{0484}', '\u{0485}', '\u{0486}', '\u{0487}', '\u{0592}', '\u{0593}', '\u{0594}', '\u{0595}',
    '\u{0597}', '\u{0598}', '\u{0599}', '\u{059C}', '\u{059D}', '\u{059E}', '\u{059F}', '\u{05A0}', '\u{05A1}', '\u{05A8}', '\u{05A9}', '\u{05AB}', '\u{05AC}',
    '\u{05AF}', '\u{05C4}', '\u{0610}', '\u{0611}', '\u{0612}', '\u{0613}', '\u{0614}', '\u{0615}', '\u{0616}', '\u{0617}', '\u{0657}', '\u{0658}', '\u{0659}',
    '\u{065A}', '\u{065B}', '\u{065D}', '\u{065E}', '\u{06D6}', '\u{06D7}', '\u{06D8}', '\u{06D9}', '\u{06DA}', '\u{06DB}', '\u{06DC}', '\u{06DF}', '\u{06E0}',
    '\u{06E1}', '\u{06E2}', '\u{06E4}', '\u{06E7}', '\u{06E8}', '\u{06EB}', '\u{06EC}', '\u{0730}', '\u{0732}', '\u{0733}', '\u{0735}', '\u{0736}', '\u{073A}',
    '\u{073D}', '\u{073F}', '\u{0740}', '\u{0741}', '\u{0743}', '\u{0745}', '\u{0747}', '\u{0749}', '\u{074A}', '\u{07EB}', '\u{07EC}', '\u{07ED}', '\u{07EE}',
    '\u{07EF}', '\u{07F0}', '\u{07F1}', '\u{07F3}', '\u{0816}', '\u{0817}', '\u{0818}', '\u{0819}', '\u{081B}', '\u{081C}', '\u{081D}', '\u{081E}', '\u{081F}',
    '\u{0820}', '\u{0821}', '\u{0822}', '\u{0823}', '\u{0825}', '\u{0826}', '\u{0827}', '\u{0829}', '\u{082A}', '\u{082B}', '\u{082C}', '\u{082D}', '\u{0951}',
    '\u{0953}', '\u{0954}', '\u{0F82}', '\u{0F83}', '\u{0F86}', '\u{0F87}', '\u{135D}', '\u{135E}', '\u{135F}', '\u{17DD}', '\u{193A}', '\u{1A17}', '\u{1A75}',
    '\u{1A76}', '\u{1A77}', '\u{1A78}', '\u{1A79}', '\u{1A7A}', '\u{1A7B}', '\u{1A7C}', '\u{1B6B}', '\u{1B6D}', '\u{1B6E}', '\u{1B6F}', '\u{1B70}', '\u{1B71}',
    '\u{1B72}', '\u{1B73}', '\u{1CD0}', '\u{1CD1}', '\u{1CD2}', '\u{1CDA}', '\u{1CDB}', '\u{1CE0}', '\u{1DC0}', '\u{1DC1}', '\u{1DC3}', '\u{1DC4}', '\u{1DC5}',
    '\u{1DC6}', '\u{1DC7}', '\u{1DC8}', '\u{1DC9}', '\u{1DCB}', '\u{1DCC}', '\u{1DD1}', '\u{1DD2}', '\u{1DD3}', '\u{1DD4}', '\u{1DD5}', '\u{1DD6}', '\u{1DD7}',
    '\u{1DD8}', '\u{1DD9}', '\u{1DDA}', '\u{1DDB}', '\u{1DDC}', '\u{1DDD}', '\u{1DDE}', '\u{1DDF}', '\u{1DE0}', '\u{1DE1}', '\u{1DE2}', '\u{1DE3}', '\u{1DE4}',
    '\u{1DE5}', '\u{1DE6}', '\u{1DFE}', '\u{20D0}', '\u{20D1}', '\u{20D4}', '\u{20D5}', '\u{20D6}', '\u{20D7}', '\u{20DB}', '\u{20DC}', '\u{20E1}', '\u{20E7}',
    '\u{20E9}', '\u{20F0}', '\u{2CEF}', '\u{2CF0}', '\u{2CF1}', '\u{2DE0}', '\u{2DE1}', '\u{2DE2}', '\u{2DE3}', '\u{2DE4}', '\u{2DE5}', '\u{2DE6}', '\u{2DE7}',
    '\u{2DE8}', '\u{2DE9}', '\u{2DEA}', '\u{2DEB}', '\u{2DEC}', '\u{2DED}', '\u{2DEE}', '\u{2DEF}', '\u{2DF0}', '\u{2DF1}', '\u{2DF2}', '\u{2DF3}', '\u{2DF4}',
    '\u{2DF5}', '\u{2DF6}', '\u{2DF7}', '\u{2DF8}', '\u{2DF9}', '\u{2DFA}', '\u{2DFB}', '\u{2DFC}', '\u{2DFD}', '\u{2DFE}', '\u{2DFF}', '\u{A66F}', '\u{A67C}',
    '\u{A67D}', '\u{A6F0}', '\u{A6F1}', '\u{A8E0}', '\u{A8E1}', '\u{A8E2}', '\u{A8E3}', '\u{A8E4}', '\u{A8E5}', '\u{A8E6}', '\u{A8E7}', '\u{A8E8}', '\u{A8E9}',
    '\u{A8EA}', '\u{A8EB}', '\u{A8EC}', '\u{A8ED}', '\u{A8EE}', '\u{A8EF}', '\u{A8F0}', '\u{A8F1}', '\u{AAB0}', '\u{AAB2}', '\u{AAB3}', '\u{AAB7}', '\u{AAB8}',
    '\u{AABE}', '\u{AABF}', '\u{AAC1}', '\u{FE20}', '\u{FE21}', '\u{FE22}', '\u{FE23}', '\u{FE24}', '\u{FE25}', '\u{FE26}', '\u{10A0F}', '\u{10A38}',
    '\u{1D185}', '\u{1D186}', '\u{1D187}', '\u{1D188}', '\u{1D189}', '\u{1D1AA}', '\u{1D1AB}', '\u{1D1AC}', '\u{1D1AD}', '\u{1D242}', '\u{1D243}', '\u{1D244}',
];

impl Kitty {
    pub fn new() -> Kitty {
        Kitty {
            ids: HashMap::new(),
            next_id: 1,
            size: HashMap::new(),
            parked: Default::default(),
            parked_bytes: 0,
            free_ids: Vec::new(),
            shown: HashMap::new(),
            tmux: false,
            virt: HashMap::new(),
            padded: HashMap::new(),
            opaque: HashMap::new(),
        }
    }

    /// For a kitty-graphics terminal seen through tmux (see the module
    /// notes).
    pub fn new_tmux() -> Kitty {
        Kitty { tmux: true, ..Kitty::new() }
    }

    /// Append a graphics command, wrapped for tmux's passthrough there
    /// (each ESC doubled inside `ESC P tmux; … ESC \`).
    fn put(&self, out: &mut Vec<u8>, cmd: &[u8]) {
        if !self.tmux {
            out.extend_from_slice(cmd);
            return;
        }
        out.extend_from_slice(b"\x1bPtmux;");
        for &b in cmd {
            if b == 0x1b {
                out.push(0x1b);
            }
            out.push(b);
        }
        out.extend_from_slice(b"\x1b\\");
    }

    /// Id of the image for `p`, sending it first if the terminal doesn't
    /// have it. Past 256 images everything is freed and resent on demand.
    fn transmit(&mut self, out: &mut Vec<u8>, p: &Placement) -> u32 {
        if let Some(&id) = self.ids.get(&p.key) {
            return id;
        }
        // (placeholders give the id as a 256-colour index: 1..=255)
        let max = if self.tmux { 255 } else { 256 };
        if self.ids.len() >= max {
            // free image memory in the terminal
            self.put(out, b"\x1b_Ga=d,d=A,q=2\x1b\\");
            self.ids.clear();
            self.shown.clear();
            self.size.clear();
            self.parked.clear();
            self.parked_bytes = 0;
            self.virt.clear();
            self.opaque.clear();
            // (from 1 again: the same ids replace the old pictures in
            // terminals that don't free them, see `free_ids`)
            self.next_id = 1;
            self.free_ids.clear();
        }
        let id = self.free_ids.pop().unwrap_or_else(|| {
            let id = self.next_id;
            self.next_id = self.next_id.wrapping_add(1).max(1);
            id
        });
        // zlib-compressed (o=z): a picture redrawn every frame (a game's
        // whole-window CBG, mostly one colour) shrinks a hundredfold, which
        // is what limits the frame rate (and over SSH, everything). Level 2,
        // not 1: flate2's zlib-rs backend writes level 1 as fixed-Huffman
        // blocks, and Ghostty 1.3's inflater crashes on a large stream of
        // those; level 2 has its own tables, is a third smaller, as fast
        let raw = p.img.as_raw();
        let mut z = flate2::write::ZlibEncoder::new(Vec::with_capacity(raw.len() / 8), flate2::Compression::new(2));
        let packed = z.write_all(raw).and_then(|_| z.finish()).ok().filter(|v| v.len() < raw.len());
        let (bytes, o) = match &packed {
            Some(v) => (v.as_slice(), ",o=z"),
            None => (raw.as_slice(), ""),
        };
        let data = base64::engine::general_purpose::STANDARD.encode(bytes);
        let chunks: Vec<&[u8]> = data.as_bytes().chunks(4096).collect();
        for (k, ch) in chunks.iter().enumerate() {
            let more = if k + 1 < chunks.len() { 1 } else { 0 };
            let mut cmd = Vec::with_capacity(ch.len() + 64);
            if k == 0 {
                let _ = write!(cmd, "\x1b_Ga=t,f=32,s={},v={},i={},q=2{},m={};", p.img.width(), p.img.height(), id, o, more);
            } else {
                let _ = write!(cmd, "\x1b_Gm={},q=2;", more);
            }
            cmd.extend_from_slice(ch);
            cmd.extend_from_slice(b"\x1b\\");
            self.put(out, &cmd);
        }
        self.size.insert(id, raw.len());
        self.ids.insert(p.key.clone(), id);
        id
    }

    /// tmux: write each placement's cells as Unicode placeholders, every
    /// frame (the renderer redraws the rows under pictures that changed,
    /// which clears old ones).
    fn draw_placeholders(&mut self, out: &mut Vec<u8>, ps: &[Placement], cell: (u32, u32)) {
        let (cw, ch) = (cell.0.max(1), cell.1.max(1));
        let mut used = Vec::new();
        for p in ps {
            // a placeholder is the cell's text: nothing can be behind it
            if p.crop_h == 0 || p.below {
                continue;
            }
            // placeholders fill whole cells: a picture that starts inside its
            // first cell is sent with that offset as transparent rows on top
            let shifted;
            let p = if p.y_off > 0 && p.crop_top == 0 {
                let key = format!("{}+y{}", p.key, p.y_off);
                if self.padded.len() > 64 {
                    self.padded.clear();
                }
                let img = self
                    .padded
                    .entry(key.clone())
                    .or_insert_with(|| {
                        let mut c = image::RgbaImage::new(p.img.width(), p.img.height() + p.y_off as u32);
                        image::imageops::overlay(&mut c, &*p.img, 0, p.y_off as i64);
                        std::rc::Rc::new(c)
                    })
                    .clone();
                shifted = Placement { key, img, y_off: 0, crop_h: p.crop_h + p.y_off as u32, ..p.clone() };
                &shifted
            } else {
                p
            };
            let id = self.transmit(out, p);
            used.push(id);
            let cols = p.img.width().div_ceil(cw).max(1);
            let rows = p.img.height().div_ceil(ch).max(1);
            if self.virt.get(&id) != Some(&(cols, rows)) {
                if self.virt.contains_key(&id) {
                    self.put(out, format!("\x1b_Ga=d,d=i,i={},q=2\x1b\\", id).as_bytes());
                }
                self.put(out, format!("\x1b_Ga=p,U=1,i={},c={},r={},q=2\x1b\\", id, cols, rows).as_bytes());
                self.virt.insert(id, (cols, rows));
                self.opaque.remove(&id);
            }
            // the image rows (whole cells) the crop leaves visible
            let opaque = self.opaque.entry(id).or_insert_with(|| opaque_cells(&p.img, cols, rows, cw, ch));
            let first = p.crop_top / ch;
            let last = (p.crop_top + p.crop_h).div_ceil(ch).min(rows);
            for r in first..last {
                let row = p.row + (r - first) as i32;
                if row < 0 || r as usize >= DIACRITICS.len() {
                    continue;
                }
                let mut at = None;
                for c in 0..(cols as usize).min(DIACRITICS.len()) {
                    // a fully transparent cell: the text under it stays
                    if !opaque.get(r as usize * cols as usize + c).copied().unwrap_or(true) {
                        continue;
                    }
                    if at != Some(c) {
                        let _ = write!(out, "\x1b[{};{}H\x1b[38;5;{}m", row + 1, p.col + 1 + c as i32, id);
                    }
                    at = Some(c + 1);
                    let mut b = [0u8; 4];
                    out.extend_from_slice('\u{10EEEE}'.encode_utf8(&mut b).as_bytes());
                    out.extend_from_slice(DIACRITICS[r as usize].encode_utf8(&mut b).as_bytes());
                    out.extend_from_slice(DIACRITICS[c].encode_utf8(&mut b).as_bytes());
                }
                if at.is_some() {
                    out.extend_from_slice(b"\x1b[39m");
                }
            }
        }
        // (every call is a whole frame: the screen's pictures are all here,
        // see `Tty::repaint_hover`)
        let was: Vec<u32> = self.virt.keys().copied().collect();
        self.park_others(&used, was);
        self.evict(out);
    }
}

impl Kitty {
    /// Free the least recently shown off-screen pictures in the terminal
    /// while those kept take more than [`PARKED_BUDGET`].
    fn evict(&mut self, out: &mut Vec<u8>) {
        while self.parked_bytes > PARKED_BUDGET {
            let Some(id) = self.parked.pop_front() else { break };
            self.parked_bytes -= self.size.remove(&id).unwrap_or(0);
            self.put(out, format!("\x1b_Ga=d,d=I,i={},q=2\x1b\\", id).as_bytes());
            self.ids.retain(|_, v| *v != id);
            self.virt.remove(&id);
            self.opaque.remove(&id);
            self.free_ids.push(id);
        }
    }

    /// The pictures in `used` are on screen; any other one that was
    /// becomes parked (kept in the terminal within the budget).
    fn park_others(&mut self, used: &[u32], was: Vec<u32>) {
        for id in used {
            if let Some(k) = self.parked.iter().position(|p| p == id) {
                self.parked.remove(k);
                self.parked_bytes -= self.size.get(id).copied().unwrap_or(0);
            }
        }
        for id in was {
            if !used.contains(&id) && !self.parked.contains(&id) {
                self.parked.push_back(id);
                self.parked_bytes += self.size.get(&id).copied().unwrap_or(0);
            }
        }
    }
}

impl Backend for Kitty {
    fn name(&self) -> &'static str {
        if self.tmux {
            "kitty-tmux"
        } else {
            "kitty"
        }
    }
    /// (inside tmux the pictures are placeholder text: text written over
    /// them takes their place)
    fn text_erases(&self) -> bool {
        self.tmux
    }
    fn draw(&mut self, out: &mut Vec<u8>, ps: &[Placement], cell: (u32, u32)) {
        if self.tmux {
            return self.draw_placeholders(out, ps, cell);
        }
        let (cw, ch) = (cell.0.max(1), cell.1.max(1));
        // this frame's placements, per image, in the order given (back to
        // front: overlapping pictures arrive merged, see `Merger`)
        let mut want: Vec<(u32, Vec<String>)> = Vec::new();
        let n_below = ps.iter().filter(|p| p.below).count() as i64;
        let mut below_k = 0i64;
        for p in ps {
            if p.crop_h == 0 || p.row < 0 {
                continue;
            }
            // background layers stack in their order, all under the text
            let z = if p.below {
                below_k += 1;
                format!(",z={}", below_k - 1 - n_below)
            } else {
                String::new()
            };
            let id = self.transmit(out, p);
            let at = match want.iter().position(|w| w.0 == id) {
                Some(k) => k,
                None => {
                    want.push((id, Vec::new()));
                    want.len() - 1
                }
            };
            // `c`/`r`: the box (in cells) the layout reserved; the terminal
            // scales the picture into it, so its size doesn't depend on our
            // guess of the cell size in pixels (Konsole on scaled displays,
            // terminals that report no pixel size)
            let cols = p.img.width().div_ceil(cw).max(1);
            let rows = p.crop_h.div_ceil(ch).max(1);
            want[at].1.push(format!(
                "\x1b[{};{}H\x1b_Ga=p,i={},q=2,C=1,y={},h={},c={},r={},Y={}{}\x1b\\",
                p.row + 1,
                p.col + 1,
                id,
                p.crop_top,
                p.crop_h,
                cols,
                rows,
                p.y_off.max(0),
                // under the text (above cell backgrounds): z < 0
                z
            ));
        }
        let keep: Vec<u32> = want.iter().map(|w| w.0).collect();
        for id in &keep {
            if let Some(k) = self.parked.iter().position(|p| p == id) {
                self.parked.remove(k);
                self.parked_bytes -= self.size.get(id).copied().unwrap_or(0);
            }
        }
        // images whose placements changed: all of that image's again
        for (id, cmds) in want {
            if self.shown.get(&id) == Some(&cmds) {
                continue;
            }
            if self.shown.contains_key(&id) {
                let _ = write!(out, "\x1b_Ga=d,d=i,i={},q=2\x1b\\", id);
            }
            for c in &cmds {
                out.extend_from_slice(c.as_bytes());
            }
            self.shown.insert(id, cmds);
        }
        // images no longer shown: taken off after the new ones are placed,
        // so a picture replaced by another (a CBG redrawn every frame) never
        // leaves the screen empty in between
        let gone: Vec<u32> = self.shown.keys().filter(|id| !keep.contains(id)).copied().collect();
        for id in gone {
            let _ = write!(out, "\x1b_Ga=d,d=i,i={},q=2\x1b\\", id);
            self.shown.remove(&id);
            self.parked.push_back(id);
            self.parked_bytes += self.size.get(&id).copied().unwrap_or(0);
        }
        // off-screen pictures are kept in the terminal only up to a budget,
        // the least recently shown freed first: a canvas redrawn every frame
        // (a new picture each time) would otherwise pile up there, hundreds
        // of megabytes a minute, while sprites shown again stay cached
        self.evict(out);
    }

    /// Forget what is on screen (after a full clear), so everything is
    /// placed again.
    fn reset(&mut self, out: &mut Vec<u8>) {
        self.put(out, b"\x1b_Ga=d,d=a,q=2\x1b\\");
        self.shown.clear();
        self.size.clear();
        self.parked.clear();
        self.parked_bytes = 0;
        self.virt.clear();
        self.opaque.clear();
    }
    fn shutdown(&mut self, out: &mut Vec<u8>) {
        self.put(out, b"\x1b_Ga=d,d=A,q=2\x1b\\");
        self.shown.clear();
        self.size.clear();
        self.parked.clear();
        self.parked_bytes = 0;
        self.virt.clear();
        self.opaque.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    fn placement(w: u32, h: u32, crop_top: u32, crop_h: u32) -> Placement {
        Placement {
            key: "k".into(),
            img: Rc::new(image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 255]))),
            col: 4,
            row: 2,
            y_off: 0,
            crop_top,
            crop_h,
            below: false,
        }
    }

    /// Inside tmux: commands in passthrough (ESC doubled), one virtual
    /// placement, and the cells as placeholders with row/column diacritics.
    /// A canvas game sends a new picture every frame: off screen they are
    /// freed past the budget, and their ids are given to the next ones, so
    /// a terminal that never frees data (Konsole) only ever holds a few.
    #[test]
    fn frames_reuse_ids() {
        let mut k = Kitty::new();
        let mut ids = std::collections::HashSet::new();
        for f in 0..40 {
            // 2000x1000 RGBA = 8 MB each, a new key every frame
            let p = Placement { key: format!("frame{}", f), img: Rc::new(image::RgbaImage::new(2000, 1000)), ..placement(20, 32, 0, 32) };
            let mut out = Vec::new();
            k.draw(&mut out, &[p], (10, 16));
            let s = String::from_utf8_lossy(&out).into_owned();
            let id = s.split("a=t,").nth(1).and_then(|t| t.split("i=").nth(1)).and_then(|t| t.split(',').next()).unwrap().to_string();
            ids.insert(id);
        }
        assert!(ids.len() <= 5, "{} distinct ids", ids.len());
        assert!(k.parked_bytes <= PARKED_BUDGET);
    }

    #[test]
    fn tmux_placeholders() {
        let mut k = Kitty::new_tmux();
        let mut out = Vec::new();
        // 20x32 px in 10x16 cells: 2 columns, 2 rows
        k.draw(&mut out, &[placement(20, 32, 0, 32)], (10, 16));
        let s = String::from_utf8(out).unwrap();
        assert!(s.starts_with("\x1bPtmux;\x1b\x1b_Ga=t,f=32,s=20,v=32,i=1,"), "{:?}", &s[..40]);
        assert!(s.contains("\x1bPtmux;\x1b\x1b_Ga=p,U=1,i=1,c=2,r=2,q=2\x1b\x1b\\\x1b\\"), "{:?}", s);
        let (p, d) = ('\u{10EEEE}', DIACRITICS);
        let row = |r: usize, y: usize| format!("\x1b[{};5H\x1b[38;5;1m{p}{}{}{p}{}{}\x1b[39m", y, d[r], d[0], d[r], d[1]);
        assert!(s.ends_with(&(row(0, 3) + &row(1, 4))), "{:?}", s);
        // the next frame: placeholders again, nothing sent again
        let mut out = Vec::new();
        k.draw(&mut out, &[placement(20, 32, 0, 32)], (10, 16));
        let s = String::from_utf8(out).unwrap();
        assert!(!s.contains("tmux;"), "{:?}", s);
        assert_eq!(s, row(0, 3) + &row(1, 4));
        // scrolled: only the second image row is left, at the top
        let mut out = Vec::new();
        k.draw(&mut out, &[placement(20, 32, 16, 16)], (10, 16));
        assert_eq!(String::from_utf8(out).unwrap(), row(1, 3));
    }

    /// A picture starting inside its first cell is sent with the offset as
    /// transparent rows on top, so its cells line up.
    #[test]
    fn tmux_sub_cell_offset() {
        let mut k = Kitty::new_tmux();
        let mut out = Vec::new();
        let p = Placement { y_off: 6, ..placement(20, 26, 0, 26) };
        k.draw(&mut out, &[p], (10, 16));
        let s = String::from_utf8(out).unwrap();
        // 26 px from 6 px down: 32 px, two rows
        assert!(s.contains("s=20,v=32,"), "{:?}", &s[..60]);
        assert!(s.contains("a=p,U=1,i=1,c=2,r=2,"), "{:?}", s);
    }

    /// A fully transparent cell gets no placeholder: the text there stays
    /// (eraAkumaMaid's title art reaches over a line with its margin).
    #[test]
    fn tmux_transparent_cells() {
        let mut img = image::RgbaImage::new(20, 32);
        // only the top-left cell shows anything
        img.put_pixel(3, 3, image::Rgba([255, 0, 0, 255]));
        let p = Placement { img: Rc::new(img), ..placement(20, 32, 0, 32) };
        let mut k = Kitty::new_tmux();
        let mut out = Vec::new();
        k.draw(&mut out, &[p], (10, 16));
        let s = String::from_utf8(out).unwrap();
        assert_eq!(s.matches('\u{10EEEE}').count(), 1, "{:?}", s);
        assert!(s.ends_with(&format!("\x1b[3;5H\x1b[38;5;1m\u{10EEEE}{}{}\x1b[39m", DIACRITICS[0], DIACRITICS[0])), "{:?}", s);
    }

    /// Outside tmux nothing changes: plain commands, no placeholders.
    #[test]
    fn plain_kitty() {
        let mut k = Kitty::new();
        let mut out = Vec::new();
        k.draw(&mut out, &[placement(20, 32, 0, 32)], (10, 16));
        let s = String::from_utf8(out).unwrap();
        assert!(s.starts_with("\x1b_Ga=t,") && !s.contains("tmux;") && !s.contains('\u{10EEEE}'), "{:?}", s);
        assert!(s.contains("\x1b[3;5H\x1b_Ga=p,i=1,q=2,C=1,y=0,h=32,c=2,r=2,Y=0\x1b\\"), "{:?}", s);
    }
}
