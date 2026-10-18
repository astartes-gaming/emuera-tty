//! No framebuffer backend outside Linux.

use super::Backend;

pub struct Fb {
    pub xres: u32,
    pub yres: u32,
}

impl Fb {
    pub fn open() -> Result<Fb, String> {
        Err("the framebuffer backend is Linux-only".into())
    }
}

impl Backend for Fb {
    fn name(&self) -> &'static str {
        "fb"
    }
}
