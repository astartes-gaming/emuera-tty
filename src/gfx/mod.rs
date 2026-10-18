//! Inline image backends: kitty graphics protocol, DEC sixel, and direct
//! Linux framebuffer blitting (for the fbcon VT / fbterm).
//!
//! The renderer works out *where* images go (cell, pixel offset, visible
//! rows) and hands each frame's [`Placement`]s to the chosen [`Backend`],
//! which only knows *how* to put pixels on that kind of terminal.

#[cfg(target_os = "linux")]
pub mod fb;
#[cfg(not(target_os = "linux"))]
#[path = "fb_none.rs"]
pub mod fb;
pub mod iterm;
pub mod kitty;
pub mod sixel;

use image::RgbaImage;
use std::rc::Rc;

/// An image to show on screen, already scaled to its pixel size.
#[derive(Clone)]
pub struct Placement {
    pub key: String,
    pub img: Rc<RgbaImage>,
    /// top-left cell (0-based); row may be negative if scrolled partly off
    pub col: i32,
    pub row: i32,
    /// vertical pixel offset inside the first cell (ypos)
    pub y_off: i32,
    /// visible pixel rows of the image: [crop_top, crop_top + crop_h)
    pub crop_top: u32,
    pub crop_h: u32,
}

/// One way of showing images. All hooks default to doing nothing.
pub trait Backend {
    /// "kitty", "iterm", "sixel", "fb" or "none" (for the startup message).
    fn name(&self) -> &'static str;
    /// Called before text is written for a frame (e.g. delete old placements).
    fn begin(&mut self, _out: &mut Vec<u8>) {}
    /// Emit escape sequences for placements (written after text).
    fn draw(&mut self, _out: &mut Vec<u8>, _p: &[Placement], _cell: (u32, u32)) {}
    /// Called after the frame has been written and drained to the tty.
    fn after_flush(&mut self, _p: &[Placement], _cell: (u32, u32)) {}
    /// The screen was cleared or may be out of step (Ctrl-L): place
    /// everything again on the next frame.
    fn reset(&mut self, _out: &mut Vec<u8>) {}
    /// Leaving: clean up anything on screen.
    fn shutdown(&mut self, _out: &mut Vec<u8>) {}
}

/// No image support: images appear as `░` placeholders of the right size.
pub struct NoGfx;
impl Backend for NoGfx {
    fn name(&self) -> &'static str {
        "none"
    }
}

/// Crop helper shared by sixel and fb backends.
/// The pixel rows `top..top+h` of `img` (for images cut off at the screen edge).
pub fn crop_rows(img: &RgbaImage, top: u32, h: u32) -> RgbaImage {
    let top = top.min(img.height());
    let h = h.min(img.height() - top);
    image::imageops::crop_imm(img, 0, top, img.width(), h.max(1)).to_image()
}
