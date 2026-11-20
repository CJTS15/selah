# Contributing to Selah

Thank you for helping. Everyone taking part follows the
[Code of Conduct](CODE_OF_CONDUCT.md).

## Reporting bugs and ideas

Open an issue with the bug report or feature request form. For a bug,
include the steps to reproduce it, your Selah version (main menu → About
Selah) and, if you can, what `flatpak run io.github.cjts15.Selah` prints in
a terminal while the problem happens.

Requests for new Bible translations go to
[selah-catalog](https://github.com/CJTS15/selah-catalog/issues).

## Translating the app

The translation template is [po/selah.pot](po/selah.pot). There is no
translation platform yet. To add a language:

1. Copy the template to `po/<language>.po` (for example `po/tl.po`) and
   translate it with a tool such as
   [Translation Editor](https://flathub.org/apps/org.gnome.Gtranslator).
2. Add the language code to `po/LINGUAS`.
3. Open a pull request.

Messages with placeholders such as `{name}` or `{reference}` must keep
them unchanged; comments starting with "Translators:" explain what they
stand for.

## Code

### Layout

| Path | What it holds |
| --- | --- |
| `src/` | The GTK app: `main.rs`, `window.rs` and one file per area in `src/window/` (search, saved, notes, read aloud, …) |
| `selah-core/` | Everything that does not need GTK: USFM import, the SQLite store and search, references, highlights and notes, the catalogue, verse numbering across translations. Unit tested. |
| `data/ui/` | Blueprint files for the window and dialogs |
| `data/translations/` | The built-in translations (USFM sources, built into SQLite at build time) |
| `build-aux/` | The Flatpak manifest and the generated Cargo sources |
| `po/` | The translation template and translations |

### Build and run

```sh
flatpak run org.flatpak.Builder --user --install --install-deps-from=flathub \
  --force-clean build-dir build-aux/io.github.cjts15.Selah.Devel.json
flatpak run io.github.cjts15.Selah.Devel
```

The Flatpak build also runs the Meson tests, which validate the desktop
file, the metainfo and the settings schema.

### Before you open a pull request

The app crate needs GTK 4.24, libadwaita 1.10 and libsoup 3, so run Cargo
inside the GNOME 51 SDK (with the Rust extension,
`org.freedesktop.Sdk.Extension.rust-stable`) if your system does not have
them:

```sh
flatpak run --filesystem="$PWD" --share=network --command=sh org.gnome.Sdk//51 -c \
  'export PATH=/usr/lib/sdk/rust-stable/bin:$PATH CARGO_TARGET_DIR=target/sdk \
     APP_ID=io.github.cjts15.Selah.Devel GETTEXT_PACKAGE=selah LOCALEDIR=/app/share/locale \
     PKGDATADIR=/app/share/selah PROFILE=development VERSION=0.1.0
   cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace'
```

Then check:

- `cargo fmt --all --check`
- Blueprint formatting:
  `blueprint-compiler format data/ui/*.blp` (add `--fix` to apply)
- If you changed `Cargo.lock`, regenerate `build-aux/cargo-sources.json`
  with
  [flatpak-cargo-generator](https://github.com/flatpak/flatpak-builder-tools/tree/master/cargo).
- If you added or changed user-visible text, mark it for translation
  (`gettext` in Rust, `_("…")` in Blueprint) and regenerate the template:
  `meson compile -C <build-dir> selah-pot`.
- Check light, dark, high contrast and a narrow (360 px) window, and that
  everything works from the keyboard.

CI runs formatting, Clippy and the `selah-core` tests, and builds the
Flatpak.

### Style

- Follow the existing code: GNOME HIG patterns, libadwaita widgets and
  named colours, no hard-coded colours outside the highlight palette.
- Give every icon-only button a tooltip and an accessible label.
- Keep pull requests small and focused, with commit subjects in the
  imperative ("Add …", "Fix …") of 72 characters or fewer. Keep
  refactoring separate from behaviour changes.

## Licence

By contributing, you agree that your contributions are licensed under the
GNU General Public License, version 3 or later.
