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

use super::{Backend, Placement};
use base64::Engine;
use std::collections::HashMap;
use std::io::Write;

pub struct Kitty {
    /// placement key (sprite + size) → image id already in the terminal
    ids: HashMap<String, u32>,
    next_id: u32,
    /// what is on screen: image id → its placement commands
    shown: HashMap<u32, Vec<String>>,
}

impl Kitty {
    pub fn new() -> Kitty {
        Kitty { ids: HashMap::new(), next_id: 1, shown: HashMap::new() }
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
            self.shown.clear();
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
    fn draw(&mut self, out: &mut Vec<u8>, ps: &[Placement], cell: (u32, u32)) {
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
        // images no longer shown
        let gone: Vec<u32> = self.shown.keys().filter(|id| !want.iter().any(|w| w.0 == **id)).copied().collect();
        for id in gone {
            let _ = write!(out, "\x1b_Ga=d,d=i,i={},q=2\x1b\\", id);
            self.shown.remove(&id);
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
    }

    /// Forget what is on screen (after a full clear), so everything is
    /// placed again.
    fn reset(&mut self, out: &mut Vec<u8>) {
        let _ = write!(out, "\x1b_Ga=d,d=a,q=2\x1b\\");
        self.shown.clear();
    }
    fn shutdown(&mut self, out: &mut Vec<u8>) {
        let _ = write!(out, "\x1b_Ga=d,d=A,q=2\x1b\\");
        self.shown.clear();
    }
}
