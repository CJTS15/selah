# Selah: plan

Last updated 2026-11-18. The full design is in [DESIGN.md](DESIGN.md).

## Decisions so far

| Topic | Decision |
| --- | --- |
| Name | **Selah** (was Biblia on 2026-11-17, switched back 2026-11-18). App ID `io.github.cjts15.Selah`, crate `selah-core`, catalog repo `selah-catalog`. |
| Tagline | "Selah: a local-first Bible for GNOME". AppStream summary: "Read the Bible offline". |
| Stack | Rust, gtk4-rs + libadwaita-rs, Blueprint, plain gtk-rs subclassing, SQLite + FTS5, Meson, Flatpak/Flathub. |
| Platform | GNOME 51 runtime (`org.gnome.Platform//51`); dev machine Fedora 45. |
| Licence | GPL-3.0-or-later. |
| Bundled text | KJV (WEB as fallback). |
| Catalog | Self-hosted `selah-catalog`: CI pulls eBible.org, keeps openly licensed texts only, publishes `catalog.json` (sha256, licence, size) on GitHub Pages, files as release assets. |
| Canon | Each translation declares its books; Deuterocanon is its own labelled section. "Show Deuterocanon" switch in Preferences (from mockups). |
| Versification | Mapping in `selah-core`; user data keyed by canonical references. |
| About | `AdwAboutDialog` lists each translation's licence, including the KJV UK Crown copyright note. |
| Networking | `soup3` on the GLib main loop, no Tokio (proposed in DESIGN.md §6; change it there if you disagree). |
| Out of scope v1 | Accounts, sync, telemetry; commentaries, lexicons, Strong's, audio; hosting copyrighted translations. |

## Open items

- [ ] **Name clash.** Another Bible app already uses "Selah". Check Flathub, GitHub and app stores, and decide before Phase 7. Keep the name centralised so a rename stays cheap.
- [ ] **Mockups say "Biblia".** Rename to Selah in the design canvas (claude.ai artifact from 2026-11-17: 9 screens, light/dark, desktop/phone) and export screenshots into `docs/mockups/`.
- [ ] Confirm the GTK version in `org.gnome.Sdk//51` and pick the matching `gtk4` crate feature.
- [ ] Decide SQLite source: runtime system SQLite vs `rusqlite` `bundled` (FTS5 must be enabled).
- [ ] Icon brief and app icon (plus symbolic icon) for GNOME Circle.

## Phase 0: scaffold

- [ ] Cargo workspace: `selah-core` (lib) and `selah` (bin), edition 2024, `rust-version` matching gtk4's MSRV (1.92).
- [ ] Pin dependencies from DESIGN.md §6 (`gtk4` 0.11, `libadwaita` 0.9 with `v1_10`, `soup3` 0.9, `rusqlite` 0.43, `gettext-rs` 0.8).
- [ ] Meson build driving Cargo (`meson.build`, `src/meson.build`, `data/meson.build`, `po/meson.build`), with `profile` option for a `.Devel` app ID.
- [ ] Blueprint: `data/ui/window.blp` with an empty `AdwApplicationWindow` + `AdwNavigationSplitView`; blueprint-compiler pinned in the manifest.
- [ ] GResource bundle and GSettings schema `io.github.cjts15.Selah.gschema.xml`.
- [ ] Flatpak manifest `build-aux/io.github.cjts15.Selah.json`: `org.gnome.Platform//51`, `org.gnome.Sdk//51`, `org.freedesktop.Sdk.Extension.rust-stable`, finish-args from DESIGN.md §12, Cargo sources via `flatpak-cargo-generator`.
- [ ] AppStream `data/io.github.cjts15.Selah.metainfo.xml.in`: summary "Read the Bible offline", description, OARS, `<launchable>`, `<url>`s, `<developer id>`; passes `appstreamcli validate`.
- [ ] Desktop file `data/io.github.cjts15.Selah.desktop.in`; passes `desktop-file-validate`.
- [ ] Placeholder icons: `hicolor/scalable/apps/io.github.cjts15.Selah.svg` and `hicolor/symbolic/apps/io.github.cjts15.Selah-symbolic.svg`.
- [ ] `po/POTFILES.in`, `po/LINGUAS`, gettext init in `main.rs`.
- [ ] Repo hygiene: README with tagline, `CODE_OF_CONDUCT.md` (GNOME CoC), `CONTRIBUTING.md`, `.editorconfig`, issue templates.
- [ ] CI (GitHub Actions): `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test -p selah-core`, Flatpak build with the GNOME 51 image, metainfo and desktop validation.
- [ ] App launches as an empty window in Flatpak on Fedora 45 with no warnings.

## Phase 1: core (`selah-core`)

- [ ] Book metadata: ids, order, section (OT/NT/DC), English names and abbreviations.
- [ ] Reference type and parser (`Jn 3:16`, `1 Cor 13`, `Ps 23:1-4`, ranges, chapter-only); localisable abbreviation tables.
- [ ] Versification schemes and mapping to the canonical scheme; tests on known divergent passages.
- [ ] USFM importer (evaluate `usfm` crate first) and OSIS importer (`quick-xml`) producing a translation package.
- [ ] Translation store: create, open read-only, list, remove; schema from DESIGN.md §7.
- [ ] FTS5 index and search API with snippets, translation and section filters.
- [ ] User DB with migrations; annotation CRUD keyed by canonical refs.
- [ ] Package format `.selah` (zip) with validation (path traversal, size caps).
- [ ] `cargo-fuzz` targets for USFM, OSIS, package unpacking, reference parser.
- [ ] Build the KJV package from source and add it to the app as bundled data.

## Phase 2: reader UI

- [ ] Window layout: split view, sidebar with Books | Saved toggle group, reader page.
- [ ] Book list with labelled Deuterocanon section; rows expand to a chapter grid.
- [ ] Reader view: verse numbers, paragraph breaks, selectable text, font size from GSettings.
- [ ] Previous/next chapter actions and shortcuts.
- [ ] Resume last position on launch.
- [ ] Adaptive phone layout via `AdwBreakpoint`; test at 360 px wide.
- [ ] Style switcher and text zoom in the main menu.

## Phase 3: search and jump

- [ ] One search entry: reference input jumps, other input searches.
- [ ] Results list with snippets, filters, keyboard navigation, `AdwStatusPage` for empty results.
- [ ] Search across all installed translations off the main thread.

## Phase 4: bookmarks, highlights, notes

- [ ] Verse actions: popover on desktop, `AdwBottomSheet` on phone.
- [ ] Five named highlight colours; bookmark; note editor.
- [ ] Saved page listing annotations, with "not in this translation" state.
- [ ] Copy verse with reference.

## Phase 5: catalog and translations

- [ ] Create `selah-catalog` repo: licence allowlist, eBible.org fetch, conversion with `selah-core`, release assets, `catalog.json` on Pages, scheduled CI.
- [ ] Catalog client with `soup3`: fetch on demand, validate schema.
- [ ] Download with progress, cancel and retry; verify size and sha256 before unpacking.
- [ ] Translations dialog: installed, available, remove, Import… via file chooser portal.
- [ ] Preferences: "Show Deuterocanon", default translation, font.

## Phase 6: polish

- [ ] About dialog with Legal section from manifests, including the KJV UK note.
- [ ] Accessibility pass: labels, focus order, Orca walkthrough, high contrast, large text.
- [ ] i18n: all strings marked, RTL translation rendering checked, Damned Lies or Weblate set up.
- [ ] Keyboard shortcuts dialog.
- [ ] Empty, error and offline states for every page.
- [ ] Screenshots (light and dark, desktop and phone) for AppStream.

## Phase 7: Flathub

- [ ] Resolve the name clash.
- [ ] Final icon.
- [ ] Tagged release, release notes in metainfo.
- [ ] Submit to Flathub (follow the current submission docs and linter).

## Phase 8: GNOME Circle

- [ ] Self-review against the Circle criteria (HIG, accessibility, metainfo, CoC, maintenance).
- [ ] Apply to GNOME Circle and address review feedback.

## v1.x backlog

- Side-by-side translations aligned by versification
- Reading plans
- GNOME Shell search provider
- Zefania XML and SWORD import
