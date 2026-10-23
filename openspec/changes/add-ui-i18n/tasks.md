## 1. Catalogue and lookup

- [ ] 1.1 `src/i18n.rs`: `.po` parser (`msgctxt`, `msgid`, `msgstr`, escapes, multi-line strings), with unit tests
- [ ] 1.2 `t!` macro: lookup in the loaded catalogue, English fallback, `{name}` placeholders filled by name; unit tests including a reordering translation
- [ ] 1.3 Catalogues `i18n/ja.po`, `zh-Hans.po`, `zh-Hant.po`, `ko.po` (headers only) compiled in with `include_str!`; one language parsed on first use, replaced when the setting changes
- [ ] 1.4 Template test: extract `t!("…")` literals from `src/`, compare with `i18n/emuera-tty.pot`, `UPDATE_POT=1` rewrites it

## 2. Language selection

- [ ] 2.1 `Language` setting (`auto`, `en`, `ja`, `zh-Hans`, `zh-Hant`, `ko`, labels in their own language) in `settings.rs` and the overlay; applies from the next screen drawn
- [ ] 2.2 Locale detection: `LC_ALL` / `LC_MESSAGES` / `LANG` on Unix and Termux, `GetUserDefaultLocaleName` on Windows (`Win32_Globalization`); Chinese region → script; unit tests for the mapping
- [ ] 2.3 `EMUERA_LANG` overrides both, and also accepts the pseudo-languages (`x-cjk`, `x-arab`, `x-cyrl`, `x-deva`, `x-avst`) for looking at layout by hand

## 3. Width and combining marks

- [ ] 3.1 `display_width` / `fit` helpers over grapheme clusters (same width rules as game text), with unit tests in Hindi, decomposed accents, Old Avestan, mixed Arabic/English
- [ ] 3.2 Combining marks kept with their base cell: side table per screen row, written after the base character; wrapping never splits a cluster; game text and interface text
- [ ] 3.3 Width audit of interface code: replace character counts used as widths (overlay title, bottom row, save manager columns, confirmation boxes, launcher list, game settings page)

## 4. Wrapping the strings

- [ ] 4.1 `tty.rs` and `tty/*.rs` (overlay, bottom row, notices, save manager, game settings page, environment variables page)
- [ ] 4.2 `picker.rs` (launcher) and `settings.rs` (setting names, values, help lines)
- [ ] 4.3 `emuconfig.rs` setting labels and `main.rs` (`--help`, error messages); the template regenerated

## 5. Tests in five scripts

- [ ] 5.1 Pseudo-translation builder: CJK, Arabic, Cyrillic, Devanagari, Old Avestan transformations of every template message, placeholders kept, ~40% longer
- [ ] 5.2 Render the overlay, launcher, save manager and confirmation boxes into an in-memory screen in each pseudo-language; check borders aligned, nothing over its space, every code point present
- [ ] 5.3 Integration check: a game line with "हिन्दी", "é" (decomposed) and Old Avestan reaches the terminal output whole

## 6. Docs and finishing

- [ ] 6.1 README (*Settings*: Language; a *Translating emuera-tty* section for translators: which file, Poedit/Weblate, how to test with `EMUERA_LANG`), man page (`Language`, `EMUERA_LANG`)
- [ ] 6.2 CLAUDE.md: interface text goes through `t!`, the template test
- [ ] 6.3 `cargo test`, `cargo clippy`, `cargo fmt --check`; Windows and big-endian checks; peak memory unchanged on AnonTW (`EMUERA_TIMING=1`)
