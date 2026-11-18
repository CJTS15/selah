# CLAUDE.md

Context for Claude Code (and humans) working on Selah.

## Project

Selah is a local-first Bible reader for GNOME: it works offline, ships the KJV,
downloads openly licensed translations from a self-hosted catalog, and keeps
bookmarks, highlights and notes on the device. Target: Flathub, then GNOME
Circle.

- App ID `io.github.cjts15.Selah` · crate `selah-core` · catalog repo `selah-catalog`
- AppStream summary: "Read the Bible offline"
- Licence: GPL-3.0-or-later

**Read first:** [docs/DESIGN.md](docs/DESIGN.md) (what and why) and
[docs/PLAN.md](docs/PLAN.md) (phased checklist; tick boxes as work lands).

## Stack

- Rust, gtk4-rs + libadwaita-rs, Blueprint UI, plain gtk-rs subclassing (no Relm4)
- SQLite with FTS5 via `rusqlite`; GSettings for preferences
- `soup3` for networking on the GLib main loop; no Tokio
- Meson drives Cargo; Flatpak with `org.gnome.Platform//51`
- Dev machine: Fedora 45, GNOME 51, Wayland, SELinux enforcing, Podman

## Layout (target)

```
selah-core/        pure Rust: refs, versification, import, store, catalog (no GTK)
selah/src/         GTK app
data/ui/*.blp      Blueprint templates
data/              metainfo, desktop file, gschema, icons, gresource
po/                gettext
build-aux/         Flatpak manifest
docs/              DESIGN.md, PLAN.md
```

## Commands (once Phase 0 lands)

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p selah-core
meson setup _build -Dprofile=development && meson compile -C _build
flatpak-builder --user --install --force-clean _flatpak build-aux/io.github.cjts15.Selah.json
flatpak run io.github.cjts15.Selah
```

## Conventions

- Keep GTK out of `selah-core`; anything testable without a display goes there.
- UI in Blueprint templates bound to Rust subclasses via `#[template]`; no UI built in code unless dynamic.
- Long work (import, indexing, hashing) on `gio::spawn_blocking`; UI futures with `glib::spawn_future_local`. Never block the main loop.
- User data is keyed by **canonical references**, never translation row IDs.
- Treat all translation files and downloads as untrusted: size limits, sha256, no path traversal, plain-text rendering only (escape before any Pango markup), parameterised SQL.
- No new `--filesystem` Flatpak permissions; use portals.
- Every user-visible string goes through gettext. Every control has an accessible label; everything is keyboard-reachable.
- Follow the current GNOME HIG and libadwaita 1.10 docs. APIs change every cycle: check the official gtk-rs, libadwaita and gjs.guide docs before using an API; don't rely on memory.
- Confirm crate versions on crates.io before adding or bumping a dependency; justify every new dependency.
- Schema changes need a numbered migration.
- Smallest reliable change; no unrequested rewrites. Update PLAN.md checkboxes and DESIGN.md when a decision changes.

## Definition of done

Works on desktop and phone widths, light and dark, empty and error states;
keyboard-accessible; no new GTK or Clippy warnings; tests pass; metainfo and
desktop file still validate.

## Open items

See "Open items" and "Risks" in docs/PLAN.md. The name Selah is final; the
clash with another Bible app called Selah is tracked as a risk only.
