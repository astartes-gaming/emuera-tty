## Context

See proposal.md for why. Constraints that shape the approach (CLAUDE.md):
it must run in 512 MB on a Raspberry Pi Zero, build for odd CPUs (no
platform-specific crates beyond `libc` / `windows-sys`), and the binary is
self-contained (the man page is compiled in). Interface strings today are
English literals spread over `tty.rs`, `tty/*.rs`, `picker.rs`,
`settings.rs`, `emuconfig.rs` and `main.rs`. Text is laid out one `char`
per cell (`tty::Cell { ch: char, w: u8, .. }`); `char_cells` gives each
character's width, and a zero-width character is skipped
(`if tw == 0 { continue; }`), which is how combining marks get lost.

## Goals / Non-Goals

**Goals:** a lookup every interface string goes through; catalogues a
translator can edit with ordinary tools; language by setting or locale;
correct widths for any script; tests in several scripts.

**Non-Goals:** shipping translations (the catalogues start empty);
translating game text, the README or the man page; bidirectional
reordering; font fallback (a terminal without an Old Avestan font shows
boxes, which is the terminal's business); the retro frontend (it is
ASCII-only by design and romanises kana already).

## Decisions

### Catalogue format: gettext `.po`, parsed by emuera-tty itself

`.po` is the long-standing standard for translating a program's messages:
GNU tools and most Linux programs use it, and every translation tool and
platform (Poedit, Weblate, Crowdin) reads it. UTF-8 throughout. The English
text is the `msgid`, so an untranslated entry falls back to English
automatically.

emuera-tty only needs `msgid` / `msgstr` pairs and `msgctxt` (to tell apart
the same English word used in two senses), so a parser of about 60 lines
(`src/i18n.rs`) does; plural forms aren't needed (Chinese, Japanese and
Korean have none, and English plurals are written as two messages).

Alternatives: **Fluent** (Mozilla; better plurals and gender, but a crate
with several dependencies, and less tooling); **gettext through libintl**
(a C library, not on Windows by default, nor buildable for every CPU here);
**JSON/TOML tables** (no translator tooling, no fuzzy-match on change).

### Catalogues compiled in, one language loaded on first use

The `.po` files are `include_str!`ed, so the binary works anywhere with no
data folder. Only the selected language is parsed, into a hash map, the
first time a message is looked up (a `OnceLock`, replaced when the setting
changes). An empty catalogue costs nothing; a full one is a few tens of KB.

### The lookup: `t!("English text", name = value, ..)`

A macro returns the translated text with `{name}` placeholders filled in.
Named placeholders let a translation reorder values. The macro's first
argument must be a literal, which is what lets the template be extracted
from the source.

The template (`i18n/emuera-tty.pot`) is produced from the source by a test:
it scans the `src/` tree for `t!("…")` literals and compares the result with
the committed template, failing with the list of differences;
`UPDATE_POT=1 cargo test` rewrites it. This needs no extra tool
(`xgettext` doesn't know Rust macros).

Alternative: an `xtask` binary; more machinery for the same result.

### Language setting and locale detection

`Language` joins the settings (`settings.rs`), stored as `auto` / `en` /
`ja` / `zh-Hans` / `zh-Hant` / `ko`. For `auto`: on Unix (and Termux)
`LC_ALL`, then `LC_MESSAGES`, then `LANG` (the POSIX order); on Windows
`GetUserDefaultLocaleName` (`windows-sys`, the `Win32_Globalization`
feature: no new crate). `zh` with `TW`, `HK`, `MO` or `Hant` is Traditional,
other `zh` Simplified.

### Width: one helper for interface text

A `display_width(&str)` (sum of `char_cells` over grapheme clusters, the
same rules as game text) replaces every `chars().count()` and `len()`
used as a width in interface code, and a `fit(&str, cols)` cuts at a
cluster boundary and pads. The overlay's own drawing (`line()` in
`settings_overlay`) already pads by width; the audit covers the rest:
titles, the bottom row's parts, the save manager's columns, confirmation
boxes, the launcher's list.

### Combining marks: kept in their base character's cell

A cell gets an optional string of the combining marks that follow its
character (`Cell.marks: Option<Box<str>>`), written right after the base
character whenever the cell is drawn. Most cells have none, so ordinary text
costs only the 16 bytes of an empty field (about 200 KB on a 200×60
screen). Wrapping happens before a base character, never between it and its
marks.

What counts as a mark: a character `unicode-width` gives width 0 that is
not a control character. That includes Devanagari vowel signs and virama,
combining diacritics, variation selectors and ZWJ, so emoji sequences also
reach the terminal whole. A cluster's width is its base character's (some
Devanagari spacing vowel signs are width 1 in `unicode-width` and keep their
own cell, which is what terminals do too).

*Changed during implementation:* the plan was a side table per screen row
(`DRow.marks`). But the hover repaint redraws single cells from a saved copy
of the frame, so marks kept outside the cell would have to be carried
through every place that copies cells; inside the cell they come along with
it.

### Right-to-left text: logical order

Text is written in storage order. Terminals that implement bidirectional
display (mlterm, recent VTE with BiDi on, Konsole partly) reorder it
themselves; reordering in emuera-tty as well would reverse it twice there.
Widths and columns are the same either way.

### Tests: pseudo-translations in five scripts

Pseudo-localisation is the usual way to test layout before translations
exist: a test builds a catalogue from the template by transforming every
message, then renders the interface with it. Five transformations, each
lengthening the text by about 40% as real translations do:

- **CJK**: each letter replaced by a CJK ideograph (two columns each);
- **Arabic**: letters mapped to Arabic letters (right-to-left);
- **Cyrillic**: letters mapped to Cyrillic lookalikes;
- **Devanagari**: syllables with vowel signs and virama (combining marks);
- **Old Avestan**: U+10B00 block (beyond the BMP, right-to-left).

Placeholders `{name}` are left intact. The checks run on the overlay, the
launcher, the save manager and confirmation boxes, drawn into an in-memory
screen (the frontend already renders to a byte buffer): every row of a box
has its border in the same column, nothing is wider than its space, and
every code point of each message appears in the output (catching dropped
marks). A few real-text unit tests cover `display_width` and wrapping
directly ("हिन्दी", "é" decomposed, Old Avestan, mixed Arabic/English).

## Risks / Trade-offs

- [A few hundred strings to wrap, in code other branches are changing] →
  done file by file in separate commits, mechanical, after the framework
  lands; the template test catches any missed `t!`.
- [Terminals disagree on some widths (Devanagari spacing vowels, emoji
  sequences)] → emuera-tty follows `unicode-width`, as it does for game text;
  the existing per-terminal width measurement (`EMUERA_MEASURE`) still
  applies to base characters.
- [Pseudo-translations can't show a translation reads well] → they test
  layout only; real translations get native speakers.
- [Old Avestan and Arabic glyphs depend on the terminal's fonts] → the tests
  check what emuera-tty writes, not how a terminal draws it.

## Migration Plan

Nothing to migrate: the default is `auto`, which gives English wherever the
catalogues are empty, so behaviour is unchanged until a translation exists.
The combining-mark change only adds characters that were dropped before.
