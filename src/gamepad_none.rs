//! No gamepads outside Linux yet (their evdev devices are Linux's): the
//! frontends see none.

// (never made here: the frontends' matches still name them)
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ev {
    Dir(u8),
    Move(i8, i8),
    A,
    B,
    Enter,
    Settings,
    Esc,
    PageUp,
    PageDown,
    ScrollUp,
    ScrollDown,
    Home,
    End,
}

pub fn start() {}

pub fn fd() -> i32 {
    -1
}

pub fn read() -> Vec<Ev> {
    Vec::new()
}

pub fn describe() -> String {
    "none (Linux only)".into()
}
