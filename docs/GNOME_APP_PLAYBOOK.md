# GNOME app playbook: Rust, Flatpak, Flathub, GNOME Circle

**Version:** 1.0 (2026-11-20) · Derived from building Selah (Nov 2026) ·
Project-agnostic: copy it into a new repo and fill in the placeholders.

This is the default process for a new native GNOME app written in Rust with
gtk4-rs, libadwaita-rs and Blueprint, shipped as a Flatpak on Flathub and
aimed at GNOME Circle. It runs in phases. Each phase ends with a gate; do not
start the next phase until the gate passes.

```
0 Idea → 1 Name, ID, licence → 2 Version baseline → 3 Design + mockups
→ 4 Plan + CLAUDE.md → 5 Scaffold + CI → 6 Milestones → 7 Polish
→ 8 Release → 9 Flathub → 10 GNOME Circle → 11 Maintenance
```

---

## Placeholders

Replace these everywhere (templates, snippets, checklists).

| Placeholder | Meaning | Example shape |
| --- | --- | --- |
| `<AppName>` | Display name, one word if possible | `Notely` |
| `<app-name>` | Lowercase project/binary/gettext name | `notely` |
| `<user>` | Lowercase code-hosting username or org | `jdoe` |
| `<app-id>` | Reverse-DNS application ID | `io.github.<user>.<AppName>` |
| `<app-path>` | Resource path of the ID | `/io/github/<user>/<AppName>/` |
| `<core-crate>` | GTK-free library crate | `<app-name>-core` |
| `<GNOME>` | Target GNOME / runtime version | `51` |
| `<GTK>`, `<ADW>` | GTK and libadwaita versions in that runtime | `4.24`, `1.10` |
| `<MSRV>` | Minimum Rust version (highest MSRV of your gtk-rs crates) | `1.92` |
| `<summary>` | AppStream summary, 10–25 characters ideally, max 35 | `Write quick notes` |
| `<repo-url>` | Public repository URL | `https://github.com/<user>/<app-name>` |

---

## Phase 0: idea and scope

Write five short answers before anything else. They become §1–§4 of the
design doc.

1. **One sentence:** what the app does, for whom. This becomes the tagline.
2. **Goals** (5–8 bullets): what v1 must do well.
3. **Non-goals for v1:** what you will refuse, in writing. This is your scope
   creep defence. Always include "no telemetry, no accounts" unless the app
   truly needs them (Circle reviewers look for this).
4. **Users** table: user type → need.
5. **Why native GNOME:** what a GNOME app gives that a web app does not
   (offline, portals, integration). If the answer is "nothing", reconsider.

**Gate:** the one-liner and non-goals are written down.

---

## Phase 1: name, app ID, licence

Decide these early: they appear in the app ID, file names, GSettings path,
resource paths, metainfo and the Flathub repo. Renaming later touches every
one of them.

### 1.1 Name

- Short, pronounceable, no "GNOME", "GTK" or "Linux" in it. Flathub
  quality guidelines: ideally ≤ 15 characters, must be < 20, no unusual
  casing or extra words.
- **Clash research** (record date and findings in the design doc):
  - Search Flathub (flathub.org) for the name and close variants.
  - Search GNOME Circle and GNOME core apps.
  - Search GitHub/GitLab/Codeberg repos and app stores (Play, App Store,
    Snap) for apps in the same category with the same name.
  - Search for a trademark in the category.
- A clash with an app in **another** category is usually fine. A clash in
  the **same** category is a risk: either rename now, or accept it and
  track it in the Risks table with a mitigation (keep the name centralised
  so a rename stays cheap; re-check before Flathub submission).
- Keep the name in one place: `meson.build` (`project()` name and
  `base_id`). Every other file gets it through `@APP_ID@`/config.

### 1.2 Application ID

Rules from the Flathub requirements page (checked 2026-11-20):

- Reverse-DNS, at least 3 components, **at most 5**, max 255 characters.
- Characters `[A-Za-z0-9_]`; dashes only in the last component. Domain
  part lowercase, dashes become underscores (`my-user` → `my_user`).
- Must not end in `.desktop`, `.app` or `.linux`.
- Code-hosting prefixes need **at least 4 components**:
  `io.github.<user>.<AppName>`, `io.gitlab.…`, `page.codeberg.…`.
  `com.github.*` / `com.gitlab.*` are **not accepted** for new apps.
- `org.gnome.*` is reserved for registered GNOME projects. Do not use it
  even if you plan to join Circle.
- If you own a domain related to the project, `<tld>.<domain>.<AppName>`
  works too, but you then must verify through
  `https://<domain>/.well-known/org.flathub.VerifiedApps.txt` or DNS.
- Last component in CamelCase matching the display name is the convention
  (`io.github.<user>.<AppName>`).
- The ID must match the metainfo `<id>`, the desktop file name, the icon
  names, the GSettings schema ID and the D-Bus name.
- Development builds use `<app-id>.Devel` so they install side by side.

### 1.3 Licence

- **Code:** `GPL-3.0-or-later` is the default (matches most GNOME apps and
  gtk-rs templates). Any OSI-approved licence is acceptable for Flathub and
  Circle; pick GPL unless you have a reason.
- **Metainfo:** `CC0-1.0` (`<metadata_license>`), as Flathub expects.
- **Bundled content** (data sets, fonts, texts, icons from elsewhere):
  confirm each one is redistributable, record its licence and attribution,
  and show it in the About dialog's Legal section. Anything with a
  "no derivatives" or unclear licence stays out.
- Put `LICENSE` in the repo root and an SPDX `license` in `Cargo.toml`
  and `meson.build`.

**Gate:** name, app ID and licence recorded in the design doc's Decisions
table, with the clash research and its date.

---

## Phase 2: version baseline

gtk-rs, libadwaita and GNOME APIs change every six-month cycle. Never write
code against versions from memory.

1. Read the GNOME release notes for developers
   (`https://release.gnome.org/<GNOME>/developers/`) and note the GTK and
   libadwaita versions.
2. Confirm what the SDK actually ships:
   ```sh
   flatpak install --user flathub org.gnome.Sdk//<GNOME> org.gnome.Platform//<GNOME> \
     org.freedesktop.Sdk.Extension.rust-stable//<freedesktop-branch>
   flatpak run --command=pkg-config org.gnome.Sdk//<GNOME> --modversion gtk4 libadwaita-1
   ```
3. On crates.io, look up the current `gtk4`, `libadwaita`, `glib`/`gio`
   and any binding crate (`soup3`, `webkit6`, …). They must all share the
   same `glib` minor version. Note each crate's MSRV.
4. Pick feature flags that match the runtime: `gtk4` `v4_<x>` and
   `libadwaita` `v1_<y>` no higher than the SDK's versions.
5. Record a **Dependency baseline** table in the design doc:

   | Crate | Version | Notes |
   | --- | --- | --- |
   | `gtk4` | `<x.y.z>` | feature `v4_<x>`; MSRV `<MSRV>` |
   | `libadwaita` | `<x.y.z>` | feature `v1_<y>` |
   | `glib`, `gio` | `<x.y>` | shared by all gtk-rs crates |

   with the line "Checked against crates.io on `<date>`. Re-check before
   bumping."
6. Every new dependency needs a one-line justification. Prefer libraries
   already in the GNOME runtime (libsoup via `soup3` instead of
   reqwest+Tokio, GSettings instead of a config crate, GLib main loop
   instead of an async runtime).

**Gate:** baseline table filled in with the date checked.

---

## Phase 3: design doc and mockups

### 3.1 `docs/DESIGN.md`

Versioned: put `Status: draft v0.N (<date>)` at the top and bump it on each
substantive change; record *what changed* in the commit message or a short
changelog line. Target audience: maintainer, contributors, Circle reviewers.

Template sections:

1. Summary (from Phase 0)
2. Goals
3. Non-goals for v1
4. Users
5. MVP features (numbered, each one testable) and "after v1" list
6. Architecture: crate layout, main widgets, async model, networking,
   settings, build. Include the dependency baseline from Phase 2.
7. Data model: where data lives (`$XDG_DATA_HOME/<app-name>/` inside the
   sandbox), schemas, migrations
8. Domain-specific sections (whatever is hard about this app)
9. UI and HIG: layout per size class, main menu, dialogs, keyboard
   shortcuts, empty and error states
10. About dialog content (credits, legal)
11. Security and privacy: Flatpak permissions with a reason for each,
    untrusted input handling, rendering, SQL, no telemetry
12. Roadmap to Flathub and Circle (phase list)
13. Risks table: risk → impact → mitigation

### 3.2 Mockups

- Every primary screen, plus empty state, error state, first run and each
  dialog.
- **Light and dark**, **desktop (≈ 1000×700) and phone (360 px wide)** for
  each screen.
- Use real libadwaita patterns only (see 3.3). Use the GNOME app icon and
  symbolic icon styles.
- Export the mockups into the repo (`docs/mockups/`) so they live with the
  code; later milestones are checked against them ("match the mockups"
  milestone).

### 3.3 HIG review of the design

Check the design against the current HIG (developer.gnome.org/hig) and the
libadwaita docs for the target version, not memory:

- Window: `AdwApplicationWindow` + `AdwToolbarView` + `AdwHeaderBar`.
- Sidebar apps: `AdwNavigationSplitView` (or `AdwOverlaySplitView`);
  collapse with `AdwBreakpoint` at narrow widths. Phone flows use
  `AdwNavigationView` pages.
- View switching: `AdwViewSwitcher`/`AdwViewStack`, or `AdwToggleGroup`
  for a small inline toggle.
- Lists: boxed lists and `AdwActionRow`/`AdwExpanderRow`/`AdwSwitchRow`.
- Dialogs: `AdwDialog` family (`AdwAlertDialog`, `AdwPreferencesDialog`,
  `AdwAboutDialog`), which become bottom sheets on phones.
  `AdwBottomSheet` for persistent phone sheets.
- Feedback: `AdwToast` for transient messages; `AdwStatusPage` for
  empty/error/offline states; modal dialogs only for destructive or
  blocking choices.
- Primary menu: style switcher (follow system/light/dark) if relevant,
  Preferences (`Ctrl+,`), Keyboard Shortcuts, About `<AppName>`.
- Standard shortcuts: `Ctrl+Q` quit, `Ctrl+W` close window, `Ctrl+F`
  search, `Ctrl+,` preferences, `Ctrl+?` shortcuts, `Ctrl+±/0` zoom.
  Do not override shortcuts that GTK widgets already use (for example
  `Alt+Left/Right` in entries, `Space` on focused buttons).
- Use named colours and the libadwaita style classes; no hard-coded
  colours except deliberate content palettes (which still need light and
  dark variants and names for accessibility).

**Gate:** DESIGN.md reviewed by the maintainer; mockups cover all screens in
light/dark and desktop/phone.

---

## Phase 4: plan and CLAUDE.md

The repo docs become the source of truth. Chats, mockup tools and notes are
inputs; the repo is what wins.

### 4.1 `docs/PLAN.md`

```markdown
# <AppName>: plan

Last updated <date>. The full design is in [DESIGN.md](DESIGN.md).

## Decisions so far
| Topic | Decision |
| --- | --- |
| Name | <AppName>, final as of <date>. App ID `<app-id>`. |
| Stack | Rust, gtk4-rs + libadwaita-rs, Blueprint, Meson, Flatpak/Flathub. |
| Platform | GNOME <GNOME> runtime. |
| Licence | GPL-3.0-or-later. |
| Out of scope v1 | … |

## Open items
- [ ] …

## Risks
- **<risk>.** <mitigation>. Full table in DESIGN.md §Risks.

## Phase 0: scaffold
- [ ] … (one checkbox per deliverable, see Phase 5 of the playbook)

## Phase 1..N: features
- [ ] …

## Phase N+1: polish / Flathub / GNOME Circle
- [ ] …

## v1.x backlog
- …
```

Rules: tick boxes in the same PR that lands the work; when a decision
changes, update the Decisions table and DESIGN.md in the same PR.

### 4.2 `CLAUDE.md` (agent and contributor context)

Short and stable. Sections:

- **Project:** one paragraph, app ID, AppStream summary, licence. "Read
  first: docs/DESIGN.md and docs/PLAN.md."
- **Stack** and dev machine.
- **Layout** (directory → purpose).
- **Commands:** fmt, clippy, tests, Meson build, Flatpak build and run.
- **Conventions** (copy and adapt):
  - Keep GTK out of `<core-crate>`; anything testable without a display
    goes there.
  - UI in Blueprint templates bound to Rust subclasses with `#[template]`;
    build UI in code only when it is dynamic.
  - Never block the main loop: blocking work on `gio::spawn_blocking`,
    UI futures with `glib::spawn_future_local`.
  - All files, downloads and imports are untrusted: size limits, checksums,
    no path traversal, parameterised SQL, plain-text rendering (escape
    before any Pango markup).
  - No `--filesystem` permissions; use portals.
  - Every user-visible string through gettext; every control has an
    accessible label; everything keyboard-reachable.
  - Check official gtk-rs, libadwaita and GNOME docs for the target
    version before using an API; confirm crate versions on crates.io
    before adding or bumping; justify every dependency.
  - Schema changes need a numbered migration.
  - Smallest reliable change; no unrequested rewrites.
- **Definition of done** (see Phase 6).

### 4.3 Private vs public docs

Decide whether `docs/` is public. If you keep planning docs local, add
`/docs/*` to `.gitignore` and re-include only what should be public
(`!/docs/GNOME_APP_PLAYBOOK.md`). Public design docs help Circle reviewers
and contributors; private ones keep the repo tidy. Either way, CLAUDE.md
must point to wherever the source of truth lives.

**Gate:** PLAN.md has a phased checklist; CLAUDE.md exists; the design and
plan are merged (or saved locally) before scaffolding starts.

---

## Phase 5: scaffold and CI

Goal: an empty window that launches from a Flatpak with no warnings, and CI
that builds it. One feature branch (`feature/m1-scaffold`), one PR.

### 5.1 Layout

```
<app-name>/
├── Cargo.toml            workspace + app crate
├── <core-crate>/         pure Rust library, no GTK, unit tested
├── src/
│   ├── main.rs           gettext init, resources, adw::Application, accels
│   ├── config.rs         constants from Meson via env!()
│   ├── window.rs         main window subclass
│   └── window/           one module per area
├── data/
│   ├── ui/*.blp          Blueprint templates
│   ├── icons/            app-icon.svg, app-icon-symbolic.svg
│   ├── screenshots/      AppStream screenshots
│   ├── <app-name>.desktop.in.in
│   ├── <app-name>.metainfo.xml.in.in
│   ├── <app-name>.gschema.xml.in
│   ├── resources.gresource.xml
│   └── style.css
├── po/                   LINGUAS, POTFILES.in, meson.build, <app-name>.pot
├── build-aux/
│   ├── <app-id>.Devel.json   development manifest (local dir source)
│   ├── <app-id>.json         release manifest
│   └── cargo-sources.json    generated by flatpak-cargo-generator
├── meson.build, meson.options
├── .github/workflows/ci.yml, ISSUE_TEMPLATE/
├── README.md, CONTRIBUTING.md, CODE_OF_CONDUCT.md, LICENSE
└── .editorconfig, .gitignore
```

### 5.2 Cargo

```toml
[package]
name = "<app-name>"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
adw = { package = "libadwaita", version = "<x.y.z>", features = ["v1_<y>"] }
gettext-rs = { version = "<x.y.z>", features = ["gettext-system"] }
gtk = { package = "gtk4", version = "<x.y.z>", features = ["v4_<x>"] }
<core-crate> = { path = "<core-crate>" }

[workspace]
members = ["<core-crate>"]

[workspace.package]
edition = "2024"
rust-version = "<MSRV>"
license = "GPL-3.0-or-later"
```

### 5.3 Meson drives Cargo

`meson.build`:

```meson
project('<app-name>', version: '0.1.0', license: 'GPL-3.0-or-later',
        meson_version: '>= 1.5.0')

i18n = import('i18n')
gnome = import('gnome')

base_id = '<app-id>'
dependency('gtk4', version: '>= <GTK>')
dependency('libadwaita-1', version: '>= <ADW>')

cargo = find_program('cargo')
glib_compile_schemas = find_program('glib-compile-schemas')
desktop_file_validate = find_program('desktop-file-validate', required: false)
appstreamcli = find_program('appstreamcli', required: false)

application_id = get_option('profile') == 'development' ? base_id + '.Devel' : base_id
gettext_package = meson.project_name()
localedir = get_option('prefix') / get_option('localedir')
pkgdatadir = get_option('prefix') / get_option('datadir') / meson.project_name()

subdir('src')
subdir('data')
subdir('po')

gnome.post_install(gtk_update_icon_cache: true, glib_compile_schemas: true,
                   update_desktop_database: true)
```

`meson.options`: a `profile` combo (`default` | `development`).

`src/meson.build`: run `cargo build` (with `--release` for the default
profile) in a `custom_target` with `build_always_stale: true`, passing
`APP_ID`, `GETTEXT_PACKAGE`, `LOCALEDIR`, `PKGDATADIR`, `PROFILE` and
`VERSION` through `env`, and copy the binary to the output.
`src/config.rs` reads them with `env!()`:

```rust
pub const APP_ID: &str = env!("APP_ID");
pub const GETTEXT_PACKAGE: &str = env!("GETTEXT_PACKAGE");
pub const LOCALEDIR: &str = env!("LOCALEDIR");
pub const PKGDATADIR: &str = env!("PKGDATADIR");
pub const PROFILE: &str = env!("PROFILE");
pub const VERSION: &str = env!("VERSION");
pub const RESOURCES_FILE: &str = concat!(env!("PKGDATADIR"), "/resources.gresource");
```

`data/meson.build`: `configure_file` + `i18n.merge_file` for the desktop file
and metainfo (output named `<application_id>.desktop` /
`.metainfo.xml`), the GSettings schema, icons renamed to the app ID, the
compiled Blueprints and `gnome.compile_resources(... gresource_bundle: true)`.
Add Meson **tests** so validation runs in every build:

```meson
test('validate-desktop', desktop_file_validate, args: [desktop_file])
test('validate-metainfo', appstreamcli, args: ['validate', '--no-net', '--explain', metainfo_file])
test('validate-schema', glib_compile_schemas, args: ['--strict', '--dry-run', meson.current_build_dir()])
```

`data/ui/meson.build`: `blueprint-compiler batch-compile` over all `.blp`
files into a `blueprints` target that the resources depend on.

### 5.4 `main.rs` essentials

- `bindtextdomain`, `bind_textdomain_codeset`, `textdomain` first.
- Load and register the GResource bundle from `RESOURCES_FILE`.
- `adw::Application` with `application_id(APP_ID)` and a **fixed**
  `resource_base_path("<app-path without trailing slash>")` so the `.Devel`
  build finds the same resources.
- On `activate`, reuse `app.active_window()` if present (second launch).
- Implement `app.quit` by closing every window instead of `app.quit()`,
  so `close-request` runs and window state is saved.
- Register accelerators with `set_accels_for_action`.

### 5.5 Data files

**Desktop file** (`data/<app-name>.desktop.in.in`):

```ini
[Desktop Entry]
Name=<AppName>
Comment=<summary>
# Translators: Search terms to find this application. Do NOT translate or localize the semicolons. The list MUST also end with a semicolon.
Keywords=<word>;<word>;
Exec=<app-name>
Icon=@APP_ID@
Terminal=false
Type=Application
Categories=<MainCategory>;<AdditionalCategory>;
StartupNotify=true
```

Use registered categories from the freedesktop menu spec. Validate with
`desktop-file-validate`.

**Metainfo** (`data/<app-name>.metainfo.xml.in.in`), minimum for Flathub
and Circle:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>@APP_ID@</id>
  <metadata_license>CC0-1.0</metadata_license>
  <project_license>GPL-3.0-or-later</project_license>
  <name><AppName></name>
  <summary><summary></summary>
  <description>
    <p>3–6 lines of about 70 characters: what it does and why.</p>
    <ul><li>Up to 10 feature bullets</li></ul>
  </description>
  <developer id="io.github.<user>">
    <name translate="no"><Developer name></name>
  </developer>
  <launchable type="desktop-id">@APP_ID@.desktop</launchable>
  <translation type="gettext">@GETTEXT_PACKAGE@</translation>
  <url type="homepage"><repo-url></url>
  <url type="bugtracker"><repo-url>/issues</url>
  <url type="vcs-browser"><repo-url></url>
  <url type="contribute"><repo-url>/blob/main/CONTRIBUTING.md</url>
  <screenshots>
    <screenshot type="default">
      <image>https://raw.githubusercontent.com/<user>/<app-name>/main/data/screenshots/main.png</image>
      <caption>One sentence, no full stop</caption>
    </screenshot>
  </screenshots>
  <branding>
    <color type="primary" scheme_preference="light">#rrggbb</color>
    <color type="primary" scheme_preference="dark">#rrggbb</color>
  </branding>
  <content_rating type="oars-1.1"/>
  <releases>
    <release version="0.1.0" date="YYYY-MM-DD">
      <description><p>First release.</p></description>
    </release>
  </releases>
  <requires><display_length compare="ge">360</display_length></requires>
  <supports>
    <control>keyboard</control>
    <control>pointing</control>
    <control>touch</control>
  </supports>
</component>
```

Fill in the OARS rating honestly (the OARS generator on
hughsie.github.io/oars helps). Validate with `appstreamcli validate --no-net`.

**GSettings schema** (`data/<app-name>.gschema.xml.in`): `id="@APP_ID@"`
`path="@APP_PATH@"`, `gettext-domain`; window width/height/maximised keys;
every key with a `<summary>`; enumerations as `<choices>`; ranges for
numbers. Validated with `glib-compile-schemas --strict`.

**Icons:** `data/icons/app-icon.svg` installed as
`share/icons/hicolor/scalable/apps/<app-id>.svg`, and
`app-icon-symbolic.svg` as `.../symbolic/apps/<app-id>-symbolic.svg`.
Follow the GNOME app icon guidelines (developer.gnome.org/hig, App Icons):
square canvas, no baked-in shadows or glows (Flathub quality rule). Use
App Icon Preview to check sizes and the symbolic variant. A placeholder is
fine until Phase 9; ask a GNOME designer or the GNOME design team's icon
requests issue tracker for the final icon early.

### 5.6 i18n

- `po/meson.build`: `i18n.gettext(gettext_package, preset: 'glib')`.
- `po/POTFILES.in`: desktop file, metainfo, every `.blp`, every `.rs` with
  strings. Keep it in sync (CI or a review checklist item).
- `po/LINGUAS`: one language code per line.
- Rust: `gettext("…")`, and for plurals `ngettext`. Use named placeholders
  (`{name}`) with `.replace()` and a `// Translators:` comment above.
- Blueprint: `_("…")`, `C_("context", "…")` for ambiguous words.
- Regenerate the template: `meson compile -C <build-dir> <app-name>-pot`.
- Translation platform: Damned Lies (GNOME l10n) only for apps hosted on
  GNOME GitLab; otherwise Hosted Weblate (free for libre projects) or
  plain PRs documented in CONTRIBUTING.md.

### 5.7 Flatpak manifest (development)

`build-aux/<app-id>.Devel.json`:

```json
{
  "id": "<app-id>.Devel",
  "runtime": "org.gnome.Platform",
  "runtime-version": "<GNOME>",
  "sdk": "org.gnome.Sdk",
  "sdk-extensions": ["org.freedesktop.Sdk.Extension.rust-stable"],
  "command": "<app-name>",
  "finish-args": [
    "--share=ipc",
    "--socket=fallback-x11",
    "--socket=wayland",
    "--device=dri"
  ],
  "build-options": {
    "append-path": "/usr/lib/sdk/rust-stable/bin",
    "env": {
      "CARGO_HOME": "/run/build/<app-name>/cargo",
      "CARGO_NET_OFFLINE": "true",
      "RUST_BACKTRACE": "1"
    }
  },
  "cleanup": ["/include", "/lib/pkgconfig", "/share/man", "*.a", "*.la"],
  "modules": [
    {
      "name": "<app-name>",
      "buildsystem": "meson",
      "config-opts": ["-Dprofile=development"],
      "run-tests": true,
      "sources": [
        { "type": "dir", "path": "..",
          "skip": ["_build", "build-dir", "repo", ".flatpak-builder", "target"] },
        "cargo-sources.json"
      ]
    }
  ]
}
```

Rules:

- **No network during the build.** Generate `cargo-sources.json` with
  `flatpak-cargo-generator` (flatpak/flatpak-builder-tools) from
  `Cargo.lock`, and regenerate it whenever `Cargo.lock` changes.
- **Build everything from source.** Extra C libraries are modules pinned
  to a tag **and** commit; disable optional features you do not need.
- **Minimal static permissions.** Each `finish-arg` gets a reason in the
  design doc's Security section. Add `--share=network` only if the app
  uses the network, `--socket=pulseaudio` only for audio. No
  `--filesystem=*`: use the file chooser, document and other portals
  (`ashpd` or `gtk::FileDialog`, which uses the portal).
- `run-tests: true` so the Meson validation tests run in every Flatpak
  build, including CI.
- Runtime must be the latest GNOME runtime on Flathub when you submit.

### 5.8 Repo hygiene

- `README.md`: tagline, screenshot, features, privacy statement,
  install/build, licence.
- `CONTRIBUTING.md`: bug reports (what to include, how to get logs with
  `flatpak run <app-id>`), translating, layout table, build and run,
  pre-PR checks, style, licence of contributions.
- `CODE_OF_CONDUCT.md`: adopt the GNOME Code of Conduct (Circle expects it).
- `.editorconfig`, issue forms (bug report, feature request).

### 5.9 CI (GitHub Actions)

```yaml
name: CI
on:
  push: { branches: [main] }
  pull_request:
permissions:
  contents: read
jobs:
  core:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v<N>
      - run: cargo fmt --all --check
      # The app crate needs the GNOME SDK's GTK/libadwaita; lint the
      # GTK-free crate here and compile the app in the Flatpak job.
      - run: cargo clippy -p <core-crate> --all-targets -- -D warnings
      - run: cargo test -p <core-crate>
  flatpak:
    runs-on: ubuntu-latest
    container:
      image: ghcr.io/flathub-infra/flatpak-github-actions:gnome-<GNOME>
      options: --privileged
    steps:
      - uses: actions/checkout@v<N>
      - uses: flatpak/flatpak-github-actions/flatpak-builder@v<N>
        with:
          manifest-path: build-aux/<app-id>.Devel.json
          bundle: <app-name>.flatpak
```

Check the current major versions of each action before copying. Lesson
learned: Ubuntu runners lag GNOME by a cycle or more, so full-workspace
Clippy must run inside the GNOME SDK (Flatpak job or
`flatpak run --command=sh org.gnome.Sdk//<GNOME>` locally).

**Gate:** `flatpak run org.flatpak.Builder --user --install
--install-deps-from=flathub --force-clean build-dir
build-aux/<app-id>.Devel.json` succeeds, the app opens an empty window with
no warnings in the terminal, Meson tests pass, CI is green.

---

## Phase 6: milestones

One milestone per feature area, each on its own branch
(`feature/m<N>-<topic>`) with one PR, merged before the next starts. Typical
order:

1. Scaffold (Phase 5)
2. Core data path end to end with bundled/sample data (core crate first,
   then UI)
3. Match the mockups (layout, typography, menus)
4. Each MVP feature: core API + tests → UI → shortcuts → empty/error states
5. Preferences
6. Polish (Phase 7)
7. Release and Flathub packaging

Per feature, the order that worked:

1. Add the logic to `<core-crate>` with unit tests (no GTK).
2. Add the Blueprint UI and the Rust subclass.
3. Wire actions and accelerators; add them to the shortcuts dialog.
4. Add the empty, error and offline states.
5. Add accessible labels and tooltips; mark strings for translation.
6. Tick the PLAN.md boxes.

### Conventions that came out of real bugs

- **Never pass untrusted text to markup.** `AdwToast` titles, `AdwActionRow`
  titles and some labels parse Pango markup by default; escape with
  `glib::markup_escape_text` or disable markup (`use-markup: false`) when
  showing user or downloaded content.
- **Untrusted databases/files:** do not open an untrusted SQLite file
  directly; copy the validated rows into a fresh database you created.
  Validate archives (no absolute paths, no `..`, size caps) before
  unpacking; verify size and SHA-256 of downloads before using them.
- **Do not cancel what is already committing:** once a download moves to
  install, stop offering Cancel.
- **Keyboard shortcuts:** check for conflicts with GTK's built-in bindings
  and with focused widgets (`Space` activates focused buttons;
  `Alt+Left/Right` and `Ctrl+Left/Right` move in text). Prefer
  `Ctrl+Page Up/Down` for previous/next.
- **gtk-rs macro paths change between releases** (for example `clone!` is
  imported from `glib` and called without the path); follow the current
  gtk-rs book and release notes when bumping.
- **RTL:** set widget direction from the content's language where content
  can be RTL, and test the UI with an RTL locale.
- **State:** save window size/maximised state in `close-request`; resume
  the last position on launch.

### Definition of done (every PR)

- Works at desktop and 360 px phone width, light and dark, high contrast,
  large text.
- Empty, loading and error states exist.
- Keyboard-reachable, logical focus order, every icon-only button has a
  tooltip and accessible label.
- No new GTK warnings in the terminal, no Clippy warnings, `cargo fmt` and
  `blueprint-compiler format` clean.
- Core tests pass; Meson validation tests pass in the Flatpak build.
- New strings are translatable and `POTFILES.in` is up to date.
- `Cargo.lock` change → `cargo-sources.json` regenerated.
- PLAN.md (and DESIGN.md if a decision changed) updated.
- Commit subjects imperative, ≤ 72 characters; refactors separate from
  behaviour changes.

---

## Phase 7: polish

- **Accessibility:** walk every screen with Orca; check focus order and
  labels (`accessibility { label: _("…"); }` in Blueprint); high contrast
  and large text; colour is never the only signal (named colours, icons
  or text too). Use Accerciser to inspect the tree.
- **Keyboard shortcuts dialog:** list every accelerator (use the dialog
  type the current libadwaita recommends).
- **About dialog:** `AdwAboutDialog` with name, icon, version, developer,
  website, issue URL, licence, credits, translator credits
  (`translator-credits` string), and a Legal section for bundled content.
- **Empty/error/offline** states for every page, with a retry action.
- **i18n:** all strings marked, template regenerated, RTL checked.
- **Screenshots:** default window size around 1000×700 (2000×1400 HiDPI),
  include the window shadow and rounded corners, English UI, 3–6 images,
  at least one dark and one phone-width if relevant, one-sentence captions
  without a full stop. Store them in `data/screenshots/` and reference
  raw URLs on the default branch (or tags) in the metainfo.
- **Branding colours:** light and dark primary colours in the metainfo,
  colourful, not white/black, distinct from each other.
- **Final icon** (see 5.5).

---

## Phase 8: release

1. Bump the version in `meson.build` and every `Cargo.toml`; update
   `Cargo.lock` and regenerate `build-aux/cargo-sources.json`.
2. Add a `<release version="X.Y.Z" date="YYYY-MM-DD">` entry with real
   notes (no "bug fixes and performance improvements") to the metainfo;
   regenerate the `.pot`.
3. Merge to `main`, then tag `vX.Y.Z` and push the tag.
4. A release workflow builds the **release manifest**
   (`build-aux/<app-id>.json`: no `.Devel`, `-Dprofile=default`, no
   `RUST_BACKTRACE`) and publishes a GitHub release with the `.flatpak`
   bundle and the metainfo notes. Make it fail if the tag does not match
   the Meson version or the release has no notes.

---

## Phase 9: Flathub

Requirements and steps from docs.flathub.org, checked 2026-11-20. Re-read
the Requirements, Submission and Quality guidelines pages before
submitting; they change.

### 9.1 Before submitting

- [ ] Re-check the name-clash risk on Flathub.
- [ ] App ID follows 1.2 and matches the metainfo `<id>`.
- [ ] App is functional, a graphical app (not a console tool, extension,
      tray-only app or thin web wrapper).
- [ ] Builds entirely from source, no network at build time, sources
      pinned (git tag + commit, or archive + sha256), patches included.
- [ ] Latest GNOME runtime on Flathub.
- [ ] Static permissions minimal, each one explained; portals used where
      they exist.
- [ ] Metainfo validates; desktop file present and valid; icon SVG (or
      PNG ≥ 256×256); complete English for UI, metainfo and desktop file.
- [ ] Quality guidelines: name < 20 chars, summary ≤ 35 chars (sentence
      case, no full stop, no leading article, does not repeat the name),
      description 3–6 lines, ≤ 10 feature bullets, screenshots and
      captions per Phase 7, branding colours, release notes.
- [ ] Single-architecture app? Add `flathub.json` (default builds x86_64
      and aarch64).

### 9.2 The Flathub manifest

The Flathub manifest lives in the Flathub app repo, not only in yours.
Top-level file named `<app-id>.json` (or `.yml`). Point the app module
at a **git tag and commit** (or a release tarball with sha256) of your
repo instead of `"type": "dir"`, and include `cargo-sources.json` next
to it. Follow the indentation and formatting rules on the Requirements
page; the linter reports violations.

### 9.3 Build and lint locally

```sh
flatpak install --user flathub org.flatpak.Builder
flatpak run --command=flathub-build org.flatpak.Builder --install <app-id>.json
flatpak run <app-id>
flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest <app-id>.json
flatpak run --command=flatpak-builder-lint org.flatpak.Builder repo repo
```

Fix every linter error. Exceptions are rare, temporary and requested
through flathub-infra/flatpak-builder-lint; warnings do not need one.

### 9.4 Submit

> **Flathub policy (checked 2026-11-20):** submission manifests must not
> contain AI-generated or AI-assisted content, and AI tools must not open
> or automate Flathub submission pull requests. Write the Flathub
> manifest and open the submission PR yourself. AI-assisted app code in
> your own repo is a separate matter; the rule is about the Flathub
> submission.

1. Fork `flathub/flathub` with "Copy the master branch only" **unchecked**.
2. `git clone --branch=new-pr <your fork>`; `git checkout -b <app-name>-submission new-pr`.
3. Add the manifest and `cargo-sources.json`, commit, push.
4. Open a PR against **`new-pr`** (not `master`) titled
   `Add <app-id>`. Explain every permission in the description.
5. Address review comments; reviewers trigger test builds with
   `bot, build`.
6. After merge you get the `flathub/<app-id>` repo; future updates are
   PRs there (the Flathub bot can open them from your releases with an
   `x-checker-data` source).

### 9.5 Verification

For `io.github.<user>.*`: log in at flathub.org with the GitHub account
that owns the repo (or an org admin), open the Developer Portal →
Verification. Custom domains verify with the `.well-known` token or a
`_flathub.<domain>` DNS TXT record.

---

## Phase 10: GNOME Circle

> Criteria **not re-verified** on 2026-11-20: gitlab.gnome.org was not
> reachable from the environment that wrote this. Read the current app
> criteria and application template in the Teams/Circle project on
> gitlab.gnome.org before applying, and update this section.

What reviewers have consistently checked (self-review before applying):

- [ ] On Flathub, with a free (OSI-approved) licence and public source.
- [ ] Uses GTK 4 and libadwaita and follows the current HIG: standard
      layouts and widgets, adaptive to phone sizes, primary menu items,
      keyboard shortcuts dialog, About dialog.
- [ ] Accessible: labels, keyboard navigation, works with Orca, high
      contrast and large text.
- [ ] Translatable UI, metainfo and desktop file; contributors can
      translate easily.
- [ ] Complete AppStream metainfo: screenshots, OARS, release notes,
      developer, URLs, branding colours.
- [ ] App icon in the GNOME style plus a symbolic icon.
- [ ] Code of Conduct (the GNOME CoC) and a CONTRIBUTING guide.
- [ ] No telemetry or tracking; no network use the user did not ask for.
- [ ] Does one job well, with a clear scope; no unfinished features
      exposed.
- [ ] Actively maintained: issues answered, runtime kept current.

Steps: open a "New app" issue in the Circle project with the template,
answer every point, then address the review feedback in PRs and reply on
the issue as each item lands. After acceptance, keep meeting the criteria
or the app can be removed.

---

## Phase 11: maintenance

- GNOME releases every March and September. For each new runtime:
  re-run Phase 2 (versions, feature flags), bump `runtime-version`, the
  gtk-rs crates and the CI container tag together, fix deprecations,
  update the baseline table and date.
- Keep `cargo-sources.json` in sync with `Cargo.lock`.
- Every release gets metainfo notes and a Flathub update.
- Re-read the Flathub Requirements page and Circle criteria once a cycle.
- Keep DESIGN.md's Risks table current; close PLAN.md items or move them
  to the backlog.

---

## Sources

Checked 2026-11-20:

- Flathub Requirements: https://docs.flathub.org/docs/for-app-authors/requirements
- Flathub Submission: https://docs.flathub.org/docs/for-app-authors/submission
- Flathub MetaInfo quality guidelines: https://docs.flathub.org/docs/for-app-authors/metainfo-guidelines/quality-guidelines
- Flathub Verification: https://docs.flathub.org/docs/for-app-authors/verification
- Flathub linter: https://docs.flathub.org/docs/for-app-authors/linter

Read before relying on the related sections (not reachable when written):

- GNOME HIG: https://developer.gnome.org/hig/
- libadwaita docs for the target version: https://gnome.pages.gitlab.gnome.org/libadwaita/doc/
- gtk-rs book and docs: https://gtk-rs.org/
- GNOME Circle: https://circle.gnome.org/ and the Teams/Circle project on gitlab.gnome.org
- GNOME release notes for developers: https://release.gnome.org/
