//! The environment-variables page, opened from the settings overlay: every
//! `EMUERA_*` variable emuera-tty reads (see [`crate::settings::ENV_VARS`]),
//! with the value it sees now or "(not set)". Read-only: the variables are
//! read when emuera-tty starts, so they are changed in the shell before it
//! is launched. Enter on a row says what the variable does. Normal users
//! shouldn't need any of them (debugging, testing, odd terminals).

use super::saves::Pick;
use super::{wrap_words, Tty};
use crate::console::Console;
use crate::frontend::InputReq;
use crate::resources::Resources;
use crate::settings::ENV_VARS;

/// `v` on one line, cut to `max` columns with a `…`.
fn short(v: &str, max: usize) -> String {
    let one: String = v.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    if crate::i18n::width(&one) > max {
        format!("{}…", crate::i18n::cut(&one, max.saturating_sub(1)).0)
    } else {
        one
    }
}

impl Tty {
    /// Run the page. Returns true when the player asked to quit.
    pub(super) fn env_panel(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq) -> bool {
        let mut sel = 0usize;
        let mut help = String::from("Normal users shouldn't need these: they are mostly for debugging and testing. ● set  ○ not set. Enter: what a variable does. They are read when emuera-tty starts, so set them in the shell before launching.");
        loop {
            let rows: Vec<String> = ENV_VARS
                .iter()
                .map(|(name, _)| match std::env::var_os(name) {
                    Some(v) => format!("● {:<24} = {}", name, short(&v.to_string_lossy(), 40)),
                    None => format!("○ {:<24}   (not set)", name),
                })
                .collect();
            // the help wraps (up to four lines, always four high so the box
            // doesn't move from one row to the next)
            let mut lines = wrap_words(&help, self.box_geo(0).2.saturating_sub(2), 4);
            lines.resize(4, String::new());
            match self.list_box(
                con,
                res,
                req,
                "Environment variables (EMUERA_*): for debugging, not normal use",
                &rows,
                rows.len(),
                &mut sel,
                &lines.join("\n"),
            ) {
                Pick::Back => return false,
                Pick::Quit => return true,
                Pick::Row(i) => {
                    let (name, what) = ENV_VARS[i];
                    help = format!("{}: {}", name, what);
                }
            }
        }
    }
}
