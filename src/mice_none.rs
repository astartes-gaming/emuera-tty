//! No console mouse outside Linux: the terminal's own mouse reports are
//! used everywhere else.

pub type Event = (u16, u16, u16, bool);

pub struct Mice;

impl Mice {
    pub fn wanted(_is_vt: bool) -> bool {
        false
    }
    pub fn open() -> Result<Mice, String> {
        Err("no console mouse on this system".into())
    }
    pub fn open_gpm() -> Result<Mice, String> {
        Self::open()
    }
    pub fn fd(&self) -> i32 {
        -1
    }
    pub fn read(&mut self, _cell: (u32, u32), _cols: usize, _rows: usize) -> Vec<Event> {
        Vec::new()
    }
}
