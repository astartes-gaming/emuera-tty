emuera-tty's own menus and messages (the settings, launcher, save manager,
prompts and its errors; never the game's text) can be translated without
touching the code. Japanese, Simplified Chinese, Traditional Chinese and
Korean have a catalogue waiting to be filled in; until a message is
translated, it shows in English.

- **The files:** `i18n/emuera-tty.pot` lists every message (it is made from
  the source, so it is always complete); each language has its own
  `i18n/<language>.po`: `ja.po`, `zh-Hans.po`, `zh-Hant.po`, `ko.po`.
  These are gettext catalogues, the usual format for translating programs.
- **The tools:** [Poedit](https://poedit.net) opens a `.po` file and, with
  *Translation → Update from POT file*, adds the messages that are new in
  the template. Weblate, Crowdin and plain text editors work too.
- **Placeholders:** `{name}` in a message is a value filled in when it is
  shown (a file, a slot number, a count). Keep each one, under the same
  name, anywhere in the sentence that suits the language. Key names
  (`Esc`, `Ctrl-O`, `F3`) and anything inside backquotes stay as they are.
- **Trying it:** after editing a catalogue, build emuera-tty
  (`cargo build --release`; the catalogues are compiled in) and run it
  with `EMUERA_LANG=ja` (or choose the language under *Settings*).
  `EMUERA_LANG=x-cjk`, `x-arab`, `x-cyrl`, `x-deva` or `x-avst` rewrites
  every message in that script and makes it longer, to see whether a
  screen still fits.
- **A new language:** copy `i18n/emuera-tty.pot` to `i18n/<code>.po`, add
  its line to `LANGUAGES` in `src/i18n.rs` and to the Language setting's
  choices in `src/settings.rs`, and send the change.

For developers: messages go through `t!("English text", name = value)`
(`tc!` with a context; `n!` marks a literal in a constant table, shown with
`i18n::tr`). The first argument must be a literal; `cargo test` fails when
the template is out of date, and `UPDATE_POT=1 cargo test` rewrites it.
