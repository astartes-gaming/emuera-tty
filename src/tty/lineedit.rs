//! The input line: the text being typed, with a cursor, and the inputs
//! given before, as Emuera's input box has them (`MainWindow`): Left /
//! Right / Home / End move in the text, Up / Down go through the last 100
//! inputs (`updateInputs`, `movePrev`), and a recalled input comes back
//! selected, so typing replaces it.

use unicode_width::UnicodeWidthChar;

/// How many inputs Emuera remembers (`prevInputs`).
const KEEP: usize = 100;

#[derive(Default)]
pub struct LineEdit {
    text: String,
    /// byte offset of the cursor in `text`
    cur: usize,
    /// the whole text is selected (a recalled input): typing replaces it
    all: bool,
    hist: History,
}

impl LineEdit {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Replace the text; the cursor goes to its end.
    pub fn set(&mut self, s: &str) {
        self.text = s.to_string();
        self.cur = self.text.len();
        self.all = false;
    }

    pub fn clear(&mut self) {
        self.set("");
    }

    /// Hand the text over (Enter), leaving the line empty.
    pub fn take(&mut self) -> String {
        let s = std::mem::take(&mut self.text);
        self.clear();
        s
    }

    /// Is the whole text selected (shown reversed)?
    pub fn selected(&self) -> bool {
        self.all && !self.text.is_empty()
    }

    /// Columns before the cursor, where the terminal's cursor goes.
    pub fn cursor_cols(&self) -> usize {
        self.text[..self.cur].chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum()
    }

    /// Columns the whole text takes.
    pub fn cols(&self) -> usize {
        self.text.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum()
    }

    pub fn insert(&mut self, c: char) {
        if std::mem::take(&mut self.all) {
            self.text.clear();
            self.cur = 0;
        }
        self.text.insert(self.cur, c);
        self.cur += c.len_utf8();
    }

    /// Backspace: the character before the cursor (or the selection).
    pub fn backspace(&mut self) {
        if std::mem::take(&mut self.all) {
            return self.clear();
        }
        if let Some(c) = self.text[..self.cur].chars().next_back() {
            self.cur -= c.len_utf8();
            self.text.remove(self.cur);
        }
    }

    /// Delete: the character under the cursor (or the selection).
    pub fn delete(&mut self) {
        if std::mem::take(&mut self.all) {
            return self.clear();
        }
        if self.cur < self.text.len() {
            self.text.remove(self.cur);
        }
    }

    /// Left: one character back; on a selection, to its start.
    pub fn left(&mut self) {
        if std::mem::take(&mut self.all) {
            self.cur = 0;
        } else if let Some(c) = self.text[..self.cur].chars().next_back() {
            self.cur -= c.len_utf8();
        }
    }

    /// Right: one character on; on a selection, to its end.
    pub fn right(&mut self) {
        if std::mem::take(&mut self.all) {
            self.cur = self.text.len();
        } else if let Some(c) = self.text[self.cur..].chars().next() {
            self.cur += c.len_utf8();
        }
    }

    pub fn home(&mut self) {
        self.all = false;
        self.cur = 0;
    }

    pub fn end(&mut self) {
        self.all = false;
        self.cur = self.text.len();
    }

    /// An input was given with Enter: remember it (Emuera's `updateInputs`).
    pub fn remember(&mut self, s: &str) {
        self.hist.update(s);
    }

    /// Up (`-1`) / Down (`1`): an earlier or later input, selected.
    pub fn recall(&mut self, step: i32) {
        if let Some(s) = self.hist.step(step) {
            self.set(&s);
            self.all = true;
        }
    }
}

/// Emuera's `prevInputs` with its two positions: `selected` is the entry
/// on the line now (KEEP: none, a new line), `last` the one recalled
/// before the last input was given, so Up after giving a recalled input
/// shows it again, and Down the one after it.
struct History {
    prev: Vec<Option<String>>,
    selected: usize,
    last: usize,
}

impl Default for History {
    fn default() -> History {
        History { prev: vec![None; KEEP], selected: KEEP, last: KEEP }
    }
}

impl History {
    /// `updateInputs`: an empty input isn't kept; one equal to the newest
    /// is kept again only when typed, not recalled.
    fn update(&mut self, cur: &str) {
        if cur.is_empty() {
            return;
        }
        if self.selected == KEEP || self.prev[KEEP - 1].as_deref() != Some(cur) {
            self.prev.remove(0);
            self.prev.push(Some(cur.to_string()));
            // (the recalled entry moved one place down with the shift)
            if self.selected > 0 && self.selected != KEEP && self.prev[self.selected - 1].as_deref() == Some(cur) {
                self.selected -= 1;
                self.last = self.selected;
            } else {
                self.last = KEEP;
            }
        } else {
            self.last = self.selected;
        }
        self.selected = KEEP;
    }

    /// `movePrev`: the text for the line, or None to leave it as it is.
    fn step(&mut self, mut step: i32) -> Option<String> {
        let mut next = if self.last != KEEP && self.selected == KEEP {
            if step == -1 {
                step = 0;
            }
            let n = self.last as i32 + step;
            self.last = KEEP;
            n
        } else {
            self.selected as i32 + step
        };
        if next < 0 || next > KEEP as i32 {
            return None;
        }
        if next == KEEP as i32 {
            self.selected = KEEP;
            return Some(String::new());
        }
        if self.prev[next as usize].is_none() {
            next += 1;
            if next == KEEP as i32 {
                return None;
            }
        }
        self.selected = next as usize;
        Some(self.prev[next as usize].clone().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::LineEdit;

    #[test]
    fn editing() {
        let mut l = LineEdit::default();
        for c in "a林c".chars() {
            l.insert(c);
        }
        l.left();
        l.left();
        assert_eq!(l.cursor_cols(), 1);
        l.insert('x');
        assert_eq!(l.text(), "ax林c");
        l.delete();
        assert_eq!(l.text(), "axc");
        l.backspace();
        assert_eq!((l.text(), l.cursor_cols()), ("ac", 1));
        l.home();
        l.backspace();
        l.end();
        l.right();
        l.insert('!');
        assert_eq!(l.take(), "ac!");
        assert!(l.is_empty());
    }

    #[test]
    fn history_as_emuera() {
        let mut l = LineEdit::default();
        // nothing given yet: Up does nothing
        l.recall(-1);
        assert_eq!(l.text(), "");
        for s in ["1", "2", "", "3"] {
            l.remember(s);
        }
        l.recall(-1);
        assert_eq!((l.text(), l.selected()), ("3", true));
        l.recall(-1);
        l.recall(-1);
        assert_eq!(l.text(), "1");
        // past the oldest: stays
        l.recall(-1);
        assert_eq!(l.text(), "1");
        l.recall(1);
        assert_eq!(l.text(), "2");
        // past the newest: an empty line
        l.recall(1);
        l.recall(1);
        assert_eq!(l.text(), "");
        // typing over a recalled input replaces it
        l.recall(-1);
        l.insert('9');
        assert_eq!(l.text(), "9");
        l.clear();
        // giving a recalled input: Up shows it again
        let mut l = given_2();
        l.recall(-1);
        assert_eq!(l.text(), "2");
        // ... and Down the one after it
        let mut l = given_2();
        l.recall(1);
        assert_eq!(l.text(), "3");
        // a typed repeat is kept again (a recalled one isn't)
        let mut l = LineEdit::default();
        l.remember("5");
        l.remember("5");
        l.recall(-1);
        l.recall(-1);
        l.recall(-1);
        assert_eq!(l.text(), "5");
    }

    /// 1, 2, 3 given, then 2 recalled and given again.
    fn given_2() -> LineEdit {
        let mut l = LineEdit::default();
        for s in ["1", "2", "3"] {
            l.remember(s);
        }
        l.recall(-1);
        l.recall(-1);
        let s = l.take();
        assert_eq!(s, "2");
        l.remember(&s);
        l
    }
}
