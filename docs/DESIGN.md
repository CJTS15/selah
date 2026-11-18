# Selah: design

**Status:** draft v0.4 (2026-11-18) · **App ID:** `io.github.cjts15.Selah` · **Licence:** GPL-3.0-or-later · **Target:** GNOME 51

> Selah is a local-first Bible reader for GNOME. It works offline, lets you
> download translations, and keeps your notes on your device.

AppStream summary: **Read the Bible offline**

Audience: the maintainer, future contributors and GNOME Circle reviewers. The
implementation plan lives in [PLAN.md](PLAN.md).

---

## 1. Summary

Selah is a native GNOME Bible reader that works fully offline from first
launch. It ships with the King James Version (KJV). Readers can download more
openly licensed translations from a curated catalog, or import files they are
entitled to use. It is written in Rust with GTK 4 and libadwaita, distributed
on Flathub, and built to meet the GNOME Circle criteria.

## 2. Goals

- Fast, distraction-free reading that works offline from first launch.
- Add translations from a curated catalog or from local files.
- Search the full text of every installed translation.
- Bookmarks, highlights and notes, stored only on the device.
- Follow the GNOME HIG and adapt to phone and desktop sizes.
- Fully keyboard-accessible, screen-reader-friendly and translatable.
- Neutral across traditions: each translation shows the books it actually has.

## 3. Non-goals for v1

- Accounts, cloud sync or telemetry of any kind.
- Commentaries, lexicons, Strong's numbers or audio.
- Hosting or redistributing copyrighted translations.

## 4. Users

| User | Need |
| --- | --- |
| Daily readers | Open the app, resume where they left off, read. |
| Students | Search, jump to references, take notes. |
| Offline users | Download once, use forever, no network needed. |
| Readers from every tradition | Catholic, Orthodox and Protestant readers each see the canon their translation contains. |

## 5. MVP features

1. **Navigation:** book, chapter and verse. Book rows expand a chapter grid in
   place.
2. **One search box** that handles both full-text search and reference jumps.
   Input that parses as a reference (`Jn 3:16`, `1 Cor 13`, `Ps 23:1-4`) jumps
   straight there; anything else runs a full-text search. Reference parsing is
   localised (book names and abbreviations per UI language).
3. **Resume** the last reading position (translation, book, chapter, scroll
   anchor verse).
4. **Full-text search** across installed translations with SQLite FTS5,
   filterable by translation and by Old Testament, New Testament or
   Deuterocanon.
5. **Bookmarks, highlights and notes.** Five named highlight colours (names,
   not only colours, for accessibility). Verse actions in a popover on desktop
   and a bottom sheet on phone.
6. **Translations:** a dedicated dialog to install or remove translations from
   the catalog (with progress and retry) and to import local files through the
   file chooser portal.
7. **Adaptive layout** for phone (360 px wide) and desktop.
8. **Accessibility:** every control labelled, full keyboard navigation, tested
   with Orca, respects system text scaling and high contrast; adjustable reader
   font size.
9. **i18n:** gettext for the UI; translation content keeps its own language and
   text direction (RTL scripts must render correctly).

**After v1 (v1.x):** two translations side by side aligned across
versification, reading plans, copy or share verses with the reference, GNOME
Shell search provider, more import formats.

## 6. Architecture

```
selah/                    Cargo workspace + Meson project
├── selah-core/           library, no GTK dependency
│   ├── refs              canonical references, parser, book metadata
│   ├── versification     scheme definitions and mapping tables
│   ├── import            USFM and OSIS parsers -> translation package
│   ├── store             SQLite: translation DBs, user DB, FTS5
│   └── catalog           catalog.json model, checksum and size checks
└── selah/                GTK app (binary)
    ├── src/              application, window, widgets (gtk-rs subclassing)
    └── data/ui/*.blp     Blueprint templates
```

- **UI:** gtk4-rs and libadwaita-rs with Blueprint templates and plain gtk-rs
  subclassing (no Relm4), which matches GNOME Circle norms and the official
  docs. Main widgets: `AdwApplicationWindow`, `AdwNavigationSplitView`,
  `AdwToolbarView`, `AdwToggleGroup` (Books | Saved), `AdwBottomSheet` (phone
  verse actions), `AdwPreferencesDialog`, `AdwAboutDialog`, `AdwToastOverlay`,
  `AdwBreakpoint`.
- **Core crate `selah-core`:** owns parsing, storage, search, catalog model and
  versification. Pure Rust, unit-tested and fuzzed on its own.
- **Async:** GLib main-context futures (`glib::spawn_future_local`) in the UI.
  Parsing, indexing and hashing run on `gio::spawn_blocking`. No Tokio.
- **Networking:** `soup3` (libsoup from the GNOME runtime). It runs on the GLib
  main loop, uses system TLS and proxy settings, and avoids a Tokio and rustls
  stack. Only the catalog client touches the network, and only on user action.
- **Settings:** GSettings (`io.github.cjts15.Selah.gschema.xml`): window size,
  style, font size, last position, "Show Deuterocanon".
- **Build:** Meson drives Cargo (GNOME Rust app template layout). Flatpak
  manifest builds offline using vendored or generated Cargo sources
  (`flatpak-cargo-generator`).

### Dependency baseline

Checked against crates.io on 2026-11-18. Re-check before bumping.

| Crate | Version | Notes |
| --- | --- | --- |
| `gtk4` | 0.11.5 | features up to `v4_24`; MSRV 1.92 |
| `libadwaita` | 0.9.2 | feature `v1_10` = libadwaita 1.10 (GNOME 51) |
| `glib`, `gio` | 0.22.x | shared by gtk4, libadwaita and soup3 |
| `soup3` | 0.9.0 | uses glib/gio 0.22 |
| `rusqlite` | 0.43.0 | MSRV 1.89; use the runtime's SQLite or `bundled` (decide in Phase 0, FTS5 required) |
| `gettext-rs` | 0.8.0 | UI translations |
| `quick-xml` | 0.42.0 | OSIS import |
| `serde`, `serde_json` | 1.0.x | catalog and package metadata |
| `sha2` | 0.11.1 | download verification |
| `thiserror` | 2.0.x | errors in `selah-core` |

GNOME 51 (released 2026-09-16) ships GTK 4.23 and libadwaita 1.10 per the
[GNOME 51 developer notes](https://release.gnome.org/51/developers/). Pick the
`gtk4` feature that matches the runtime's actual GTK version
(`flatpak run --command=pkg-config org.gnome.Sdk//51 --modversion gtk4`).

USFM parsing: evaluate the `usfm` crate (0.2.x) against eBible.org files
before writing a parser; keep the parser behind a `selah-core` trait either
way.

## 7. Data model

All data lives under the Flatpak's `$XDG_DATA_HOME/selah/`.

```
translations/<id>/translation.sqlite   one DB per translation (read-only after install)
translations/<id>/manifest.json        id, name, lang, dir, licence, attribution, canon, versification, version, sha256
user.sqlite                            bookmarks, highlights, notes (never deleted with a translation)
```

**Translation DB**

```sql
CREATE TABLE book   (id TEXT PRIMARY KEY,      -- OSIS/USFM id, e.g. 'JHN', 'TOB'
                     ord INTEGER NOT NULL, section TEXT NOT NULL, -- 'OT' | 'NT' | 'DC'
                     name TEXT NOT NULL, abbrev TEXT NOT NULL);
CREATE TABLE verse  (book TEXT NOT NULL, chapter INTEGER NOT NULL, verse INTEGER NOT NULL,
                     text TEXT NOT NULL,       -- plain text, never markup
                     UNIQUE (book, chapter, verse));
-- external-content FTS5 index over verse.text (keeps snippet()/highlight() available)
CREATE VIRTUAL TABLE verse_fts USING fts5(text, content='verse', content_rowid='rowid',
                     tokenize='unicode61 remove_diacritics 2');
```

Headings, paragraphs and footnotes are stored as structured spans in a
separate table when the importer supports them; MVP renders verse text only
plus paragraph breaks.

**User DB:** keyed by **canonical references**, never row IDs, so data
survives removing a translation and switching translations.

```sql
CREATE TABLE annotation (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('bookmark','highlight','note')),
  ref_start TEXT NOT NULL,   -- canonical ref, e.g. 'JHN.3.16' in the canonical scheme
  ref_end   TEXT NOT NULL,
  colour TEXT,               -- highlight colour name
  body TEXT,                 -- note text
  created INTEGER NOT NULL, modified INTEGER NOT NULL);
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT);   -- schema_version for migrations
```

Schema changes go through numbered migrations in `selah-core::store`.

## 8. Canon and versification

- Each translation declares its **books** and its **versification scheme**
  (KJV, English, Catholic, Orthodox, LXX, Vulgate and so on, following the
  schemes used by USFM/Paratext and SWORD).
- The book list shows only the current translation's books. The Deuterocanon
  or Apocrypha appears as its own **labelled section**, never mixed into the
  Old Testament list.
- Preferences has a **"Show Deuterocanon"** switch (from the mockups). It only
  hides that section for translations that include it; it never changes the
  text or the stored data.
- `selah-core::versification` maps any translation's reference to a single
  **canonical scheme** and back. Annotations are stored canonically and mapped
  to the open translation for display. Unmappable verses keep the annotation
  and show it in Saved with a "not in this translation" state.

## 9. Catalog (`selah-catalog` repo)

A separate repository whose CI builds and publishes translations.

1. A scheduled CI job reads eBible.org's metadata.
2. It keeps only **redistributable** licences (public domain, CC BY, CC BY-SA
   and similar; no "no derivatives" licence if conversion counts as a
   derivative). The allowlist is in the repo and reviewed by hand.
3. It converts each text with the same `selah-core` importer into a
   translation package (`.selah` = zip of `translation.sqlite` +
   `manifest.json`).
4. It attaches packages to a GitHub Release as assets.
5. It publishes `catalog.json` on GitHub Pages:

```json
{
  "schema": 1,
  "generated": "2026-11-18T00:00:00Z",
  "translations": [{
    "id": "eng-web", "name": "World English Bible", "language": "en", "direction": "ltr",
    "licence": "Public Domain", "attribution": "…", "version": "2026.11",
    "canon": ["OT","NT","DC"], "versification": "English",
    "url": "https://github.com/CJTS15/selah-catalog/releases/download/…/eng-web.selah",
    "size": 4718592, "sha256": "…"
  }]
}
```

The app fetches the catalog only when the Translations dialog opens or the user
refreshes. Downloads must be HTTPS, must match `size` and `sha256`, and have a
hard size cap before unpacking.

## 10. UI and HIG

Mockups (9 screens, light and dark, desktop and phone) exist from 2026-11-17.
They still say "Biblia" and need renaming to Selah.

- **Desktop:** `AdwNavigationSplitView`. Sidebar: header with the search entry,
  `AdwToggleGroup` Books | Saved. Content: reader in an `AdwToolbarView` with
  translation picker and chapter navigation.
- **Phone:** the split view collapses via `AdwBreakpoint`; books list, reader
  and search are navigation pages. Verse actions open in `AdwBottomSheet`.
- **Main menu:** style switcher (follow system / light / dark), text zoom,
  Translations, Preferences, Keyboard Shortcuts, About Selah.
- **Translations dialog:** installed list, catalog list with size and licence,
  per-item progress and retry, **Import…** through the file chooser portal.
- **Keyboard:** `Ctrl+F`/`Ctrl+L` focus search, `Ctrl+D` bookmark,
  `Alt+Left/Right` previous/next chapter, `Ctrl+Plus/Minus/0` zoom,
  `Ctrl+?` shortcuts dialog, `Ctrl+,` preferences.
- **Empty and error states:** `AdwStatusPage` for no results, no saved items,
  catalog offline (with retry); errors shown as toasts, never modal unless
  destructive.
- Follow the current HIG and libadwaita 1.10 docs; check
  [gjs.guide](https://gjs.guide) and the libadwaita docs each cycle because
  APIs change.

## 11. About dialog

`AdwAboutDialog` with app name, icon, version, developer, website, issue
tracker, GPL-3.0-or-later and credits. A **Legal** section lists each installed
translation's name, licence and attribution, generated from the manifests. The
KJV entry includes:

> The King James Version is in the public domain in most of the world. In the
> United Kingdom, rights are vested in the Crown and administered by the
> King's Printer under Letters Patent.

## 12. Security and privacy

- **Flatpak sandbox:** `--share=network` (catalog and downloads only),
  `--socket=wayland`, `--socket=fallback-x11`, `--share=ipc`,
  `--device=dri`. No `--filesystem` access: import goes through the file
  chooser portal.
- **Downloads:** HTTPS only, size check and SHA-256 check before unpacking,
  zip entries validated (no path traversal, no absolute paths, size limits).
- **Parsers** treat all input as untrusted; fuzzed with `cargo-fuzz`.
- **Rendering:** translation text and notes are always set as plain text,
  never Pango markup without escaping.
- **SQL:** parameterised queries only; FTS queries built from tokenised input,
  not raw user strings.
- **Privacy:** no telemetry, no accounts, no network until the user asks.

## 13. Roadmap toward Flathub and GNOME Circle

1. Scaffold and CI (Phase 0).
2. Core: import, storage, search, versification (Phase 1).
3. Reader UI with bundled KJV (Phase 2).
4. Search box and reference jumps (Phase 3).
5. Saved: bookmarks, highlights, notes (Phase 4).
6. Catalog repo and Translations dialog (Phase 5).
7. Polish: accessibility, i18n, About, screenshots (Phase 6).
8. Flathub submission (Phase 7), then GNOME Circle application (Phase 8).

GNOME Circle expectations: public repo, OSI licence, code of conduct, HIG
compliance, AppStream metainfo with screenshots and OARS rating, app icon plus
symbolic icon, translatable UI, accessible, on Flathub, no telemetry,
maintained.

## 14. Risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Name clash: another Bible app uses "Selah" | Flathub review friction, user confusion | Check Flathub, GitHub and app stores before Phase 7; keep the name in one place (`meson.build` + app ID) so a rename is cheap. |
| Translation licensing | Takedown, Flathub rejection | Allowlist of licences in `selah-catalog`; licence shown per translation; never host copyrighted texts. |
| KJV Crown copyright in the UK | Legal note required | Shown in About; WEB as fallback bundled text. |
| Versification mapping errors | Annotations attached to wrong verses | Use established mapping data, store canonically, test with known divergent passages (Ps titles, Mal 4, Joel 2–3, 3 John 14–15, Dan 3 additions). |
| Malformed or hostile import files | Crash, resource exhaustion | Fuzzing, size limits, plain-text rendering. |
| gtk-rs and libadwaita API churn each cycle | Build breaks on runtime bump | Pin crate versions, bump with the runtime once per GNOME cycle. |
| Search quality across languages | Poor results for non-Latin scripts | FTS5 `unicode61` with diacritics folding; test RTL and CJK; consider trigram tokenizer later. |
| Scope creep (study tools) | Delays v1 | Non-goals list; v1.x backlog. |
