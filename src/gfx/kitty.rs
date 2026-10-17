//! kitty graphics protocol backend (also works in WezTerm, Ghostty, Konsole).
//!
//! Each distinct scaled image is transmitted once (`a=t`, RGBA, base64 in
//! 4 KiB chunks) under a numeric id; every frame then only deletes the old
//! *placements* and re-places images by id (`a=p`), which is cheap. `C=1`
//! keeps the text cursor still; `y`/`h` crop, `c`/`r` give the size in
//! cells, `Y` offsets inside the cell.

use super::{Backend, Placement};
use base64::Engine;
use std::collections::HashMap;
use std::io::Write;

pub struct Kitty {
    /// placement key (sprite + size) → image id already in the terminal
    ids: HashMap<String, u32>,
    next_id: u32,
}

impl Kitty {
    pub fn new() -> Kitty {
        Kitty { ids: HashMap::new(), next_id: 1 }
    }

    /// Id of the image for `p`, sending it first if the terminal doesn't
    /// have it. Past 256 images everything is freed and resent on demand.
    fn transmit(&mut self, out: &mut Vec<u8>, p: &Placement) -> u32 {
        if let Some(&id) = self.ids.get(&p.key) {
            return id;
        }
        if self.ids.len() > 256 {
            // free image memory in the terminal
            let _ = write!(out, "\x1b_Ga=d,d=A,q=2\x1b\\");
            self.ids.clear();
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let data = base64::engine::general_purpose::STANDARD.encode(p.img.as_raw());
        let chunks: Vec<&[u8]> = data.as_bytes().chunks(4096).collect();
        for (k, ch) in chunks.iter().enumerate() {
            let more = if k + 1 < chunks.len() { 1 } else { 0 };
            if k == 0 {
                let _ = write!(out, "\x1b_Ga=t,f=32,s={},v={},i={},q=2,m={};", p.img.width(), p.img.height(), id, more);
            } else {
                let _ = write!(out, "\x1b_Gm={},q=2;", more);
            }
            out.extend_from_slice(ch);
            out.extend_from_slice(b"\x1b\\");
        }
        self.ids.insert(p.key.clone(), id);
        id
    }
}

impl Backend for Kitty {
    fn name(&self) -> &'static str {
        "kitty"
    }
    fn begin(&mut self, out: &mut Vec<u8>) {
        // delete all placements, keep image data
        let _ = write!(out, "\x1b_Ga=d,d=a,q=2\x1b\\");
    }
    fn draw(&mut self, out: &mut Vec<u8>, ps: &[Placement], cell: (u32, u32)) {
        let (cw, ch) = (cell.0.max(1), cell.1.max(1));
        for p in ps {
            if p.crop_h == 0 || p.row < 0 {
                continue;
            }
            let id = self.transmit(out, p);
            // `c`/`r`: the box (in cells) the layout reserved; the terminal
            // scales the picture into it, so its size doesn't depend on our
            // guess of the cell size in pixels (Konsole on scaled displays,
            // terminals that report no pixel size)
            let cols = p.img.width().div_ceil(cw).max(1);
            let rows = p.crop_h.div_ceil(ch).max(1);
            let _ = write!(
                out,
                "\x1b[{};{}H\x1b_Ga=p,i={},q=2,C=1,y={},h={},c={},r={},Y={}\x1b\\",
                p.row + 1,
                p.col + 1,
                id,
                p.crop_top,
                p.crop_h,
                cols,
                rows,
                p.y_off.max(0)
            );
        }
    }
    fn shutdown(&mut self, out: &mut Vec<u8>) {
        let _ = write!(out, "\x1b_Ga=d,d=A,q=2\x1b\\");
    }
}
