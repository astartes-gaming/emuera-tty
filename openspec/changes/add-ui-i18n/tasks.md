## 1. Catalogue and lookup

- [x] 1.1 `src/i18n.rs`: `.po` parser (`msgctxt`, `msgid`, `msgstr`, escapes, multi-line strings). Verify: unit tests parse a sample with each construct, and an entry with an empty `msgstr` falls back to its `msgid`.
- [x] 1.2 `t!` macro: lookup in the loaded catalogue, English fallback, `{name}` placeholders filled by name. Verify: unit tests, including a translation that reorders two placeholders and one that leaves a placeholder out.
- [x] 1.3 Catalogues `i18n/ja.po`, `zh-Hans.po`, `zh-Hant.po`, `ko.po` (headers only), compiled in with `include_str!`; one language parsed on first use and replaced when the setting changes. Verify: a unit test switches language twice and gets each catalogue's text, and an empty catalogue gives English.
- [x] 1.4 Template test: extract `t!("…")` literals from `src/`, compare them with `i18n/emuera-tty.pot`, and have `UPDATE_POT=1` rewrite it. Verify: adding a `t!` without updating the template fails `cargo test` and names the message; `UPDATE_POT=1 cargo test` fixes it.
- [x] 1.5 CLAUDE.md: interface text goes through `t!`, and the template test. Verify: the section names the macro, the template file and `UPDATE_POT=1`.

## 2. Language selection

- [ ] 2.1 `Language` setting (`auto`, `en`, `ja`, `zh-Hans`, `zh-Hant`, `ko`, each label in its own language) in `settings.rs` and the overlay, applied from the next screen drawn. Verify: a unit test round-trips the setting through the config file, and choosing 日本語 with a test catalogue changes the overlay's title on the next frame.
- [x] 2.2 Locale detection: `LC_ALL` / `LC_MESSAGES` / `LANG` on Unix and Termux, `GetUserDefaultLocaleName` on Windows (`Win32_Globalization`), with Chinese region mapped to script. Verify: unit tests for `ja_JP.UTF-8`, `zh_TW`, `zh_HK`, `zh_CN`, `zh-Hant`, `ko_KR`, `C`, `POSIX` and an empty value; `cargo check --target x86_64-pc-windows-gnu` builds.
- [x] 2.3 `EMUERA_LANG` overrides both, and also accepts the pseudo-languages (`x-cjk`, `x-arab`, `x-cyrl`, `x-deva`, `x-avst`) for looking at layout by hand. Verify: a unit test of the precedence (setting `en` + `EMUERA_LANG=ja` gives Japanese).
- [x] 2.4 README *Settings* (Language) and man page (`Language`, `EMUERA_LANG`). Verify: `emuera-tty --manual` shows both.

## 3. Width and combining marks

- [x] 3.1 `display_width` / `fit` helpers over grapheme clusters, with the same width rules as game text. Verify: unit tests in Hindi ("हिन्दी"), decomposed "é", Old Avestan, and mixed Arabic/English; `fit` never cuts inside a cluster.
- [x] 3.2 Combining marks kept with their base cell: a side table per screen row, written after the base character; wrapping never splits a cluster; applies to game text and interface text. Verify: a test game printing "हिन्दी", a decomposed "é" and Old Avestan reaches the TUI's output with every code point, in order; the same with `--headless`.
- [ ] 3.3 Width audit of interface code: replace character counts used as widths (overlay title, bottom row, save manager columns, confirmation boxes, launcher list, game settings page). Verify: with `EMUERA_LANG=x-cjk` (or a CJK test string where 5.1 isn't done yet), each of those screens keeps its borders aligned in an 80-column pty capture.
- [x] 3.4 `doc/differences.md`, or the README, notes combining marks and right-to-left text in logical order. Verify: the note says what a terminal without BiDi shows.

## 4. Wrapping the strings

- [x] 4.1 `tty.rs` and `tty/*.rs` (overlay, bottom row, notices, save manager, game settings page, environment variables page). Verify: the template test lists the new messages, and English screens are unchanged (the existing tests pass, and a pty capture of the overlay matches the one from before).
- [x] 4.2 `picker.rs` (launcher) and `settings.rs` (setting names, values, help lines). Verify: as 4.1, for the launcher and the overlay's settings.
- [x] 4.3 `emuconfig.rs` setting labels and `main.rs` (`--help`, error messages); the template regenerated. Verify: as 4.1, and `--help` output is unchanged in English.

## 5. Tests in five scripts

- [x] 5.1 Pseudo-translation builder: CJK, Arabic, Cyrillic, Devanagari and Old Avestan transformations of every template message, placeholders kept, about 40% longer. Verify: unit tests check each script's code-point range, the kept placeholders and the length ratio.
- [ ] 5.2 Render the overlay, launcher, save manager and confirmation boxes into an in-memory screen in each pseudo-language, and check that borders line up, nothing overflows its space, and every code point is present. Verify: the tests pass, and break when a width call is reverted to `chars().count()`.

## 6. Integration check

- [ ] 6.1 `cargo test`, `cargo clippy`, `cargo fmt --check`, a Windows cross-check, and a big-endian check (`cargo check --target powerpc64-unknown-linux-gnu` or under qemu); peak memory unchanged on AnonTW (`EMUERA_TIMING=1`). Verify: all pass, and peak memory is within 1 MB of the master build.
- [ ] 6.2 README *Translating emuera-tty* section for translators: which file, Poedit or Weblate, how to test with `EMUERA_LANG`. Verify: following it, a test translation of one message shows up in the overlay.

## Workflow follow-up

- Archive the change with `openspec archive add-ui-i18n` once merged upstream.
- Translations themselves come from native speakers, in later changes.
