//! The Translations dialog: installed translations, importing `.selah`
//! packages and removing imported translations.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use selah_core::store;

use super::*;

/// Largest package accepted, and the most it may unpack to (the KJV is
/// about 10 MB). The unpacked size is counted while reading, so a gzip bomb
/// stops at the cap.
const MAX_PACKAGE: u64 = 64 * 1024 * 1024;
const MAX_UNPACKED: u64 = 256 * 1024 * 1024;

#[derive(Debug)]
pub struct TranslationsDialog {
    pub(super) dialog: adw::Dialog,
    pub(super) list: gtk::ListBox,
    pub(super) toasts: adw::ToastOverlay,
    pub(super) available_stack: gtk::Stack,
    pub(super) available_list: gtk::ListBox,
    pub(super) available_message: gtk::Label,
    pub(super) language_filter: gtk::DropDown,
    /// Each Available row and its button, updated in place during downloads.
    pub(super) available_rows: RefCell<HashMap<String, (adw::ActionRow, gtk::Button)>>,
}

/// Why an import failed, worded for the person importing.
#[derive(Debug)]
pub(super) enum ImportError {
    NotPackage,
    TooLarge,
    BuiltIn(String),
    Invalid(String),
    Io(String),
}

impl ImportError {
    pub(super) fn message(&self) -> String {
        match self {
            Self::NotPackage => {
                gettext("The file is not a Selah translation package, or it is damaged.")
            }
            Self::TooLarge => gettext("The file is too large to be a translation."),
            // Translators: {name} is a translation name.
            Self::BuiltIn(name) => {
                gettext("{name} is built in and cannot be replaced.").replace("{name}", name)
            }
            // Translators: {detail} is a technical description of the problem.
            Self::Invalid(detail) => gettext("The translation in this file is not valid: {detail}")
                .replace("{detail}", detail),
            // Translators: {detail} is a system error message.
            Self::Io(detail) => {
                gettext("The translation could not be saved: {detail}").replace("{detail}", detail)
            }
        }
    }
}

fn io_error(err: impl std::fmt::Display) -> ImportError {
    ImportError::Io(err.to_string())
}

/// What an import installed.
#[derive(Debug)]
pub(super) struct Installed {
    id: String,
    name: String,
    replaced: bool,
}

/// Unpacks a gzip-compressed package into `out`, within the size limits.
fn unpack(file: &gio::File, out: &Path) -> Result<(), ImportError> {
    let info = file
        .query_info(
            gio::FILE_ATTRIBUTE_STANDARD_SIZE,
            gio::FileQueryInfoFlags::NONE,
            gio::Cancellable::NONE,
        )
        .map_err(io_error)?;
    if u64::try_from(info.size()).unwrap_or(u64::MAX) > MAX_PACKAGE {
        return Err(ImportError::TooLarge);
    }
    let input = file.read(gio::Cancellable::NONE).map_err(io_error)?;
    let converter = gio::ZlibDecompressor::new(gio::ZlibCompressorFormat::Gzip);
    let mut reader = gio::ConverterInputStream::new(&input, &converter).into_read();
    let mut output = fs::File::create_new(out).map_err(io_error)?;
    let mut buffer = vec![0; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        // Not gzip, or cut short.
        let n = reader
            .read(&mut buffer)
            .map_err(|_| ImportError::NotPackage)?;
        if n == 0 {
            return Ok(());
        }
        total += n as u64;
        if total > MAX_UNPACKED {
            return Err(ImportError::TooLarge);
        }
        output.write_all(&buffer[..n]).map_err(io_error)?;
    }
}

/// Unpacks and checks a package, then installs a rebuilt copy in `dir` as
/// `<id>.sqlite`. Temporary files are removed whatever happens.
pub(super) fn install_package(
    file: &gio::File,
    dir: &Path,
    built_in: &[(String, String)],
) -> Result<Installed, ImportError> {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    fs::create_dir_all(dir).map_err(io_error)?;
    // Neither name ends in .sqlite, so a leftover is never listed.
    let stem = format!(
        ".import-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let unpacked = dir.join(format!("{stem}.part"));
    let rebuilt = dir.join(format!("{stem}.new"));

    let result = (|| {
        unpack(file, &unpacked)?;
        let meta = store::rebuild(&unpacked, &rebuilt).map_err(|err| match err {
            selah_core::Error::Sqlite(_) => ImportError::NotPackage,
            other => ImportError::Invalid(other.to_string()),
        })?;
        let value = |key: &str| {
            meta.iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        let (id, name) = (value("id"), value("name"));
        if let Some((_, name)) = built_in.iter().find(|(built_in, _)| *built_in == id) {
            return Err(ImportError::BuiltIn(name.clone()));
        }
        // rebuild checked the id, so it is a plain file name.
        let target = dir.join(format!("{id}.sqlite"));
        let replaced = target.exists();
        fs::rename(&rebuilt, &target).map_err(io_error)?;
        Ok(Installed { id, name, replaced })
    })();
    // SQLite may add -wal/-shm files next to a WAL-mode package it opened.
    for path in [&unpacked, &rebuilt] {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let mut name = path.clone().into_os_string();
            name.push(suffix);
            let _ = fs::remove_file(name);
        }
    }
    result
}

/// A row of the Installed list: radio to open, abbreviation, Remove.
fn translation_row(info: &TranslationInfo) -> adw::ActionRow {
    let mut details: Vec<String> = [&info.language, &info.licence]
        .into_iter()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect();
    if info.deuterocanon {
        details.push(gettext("Includes Deuterocanon"));
    }
    details.push(if info.built_in {
        gettext("Built in")
    } else {
        glib::format_size(info.size).to_string()
    });

    // Translation metadata is plain text, never markup.
    let row = adw::ActionRow::builder()
        .title(&info.name)
        .subtitle(details.join(" · "))
        .use_markup(false)
        .build();
    // A radio bound to the picker's action: it shows and sets the open translation.
    let radio = gtk::CheckButton::builder()
        .action_name("win.translation")
        .action_target(&info.id.to_variant())
        .valign(gtk::Align::Center)
        .build();
    radio.update_property(&[gtk::accessible::Property::Label(&info.name)]);
    row.add_prefix(&radio);
    row.set_activatable_widget(Some(&radio));
    row.add_suffix(
        &gtk::Label::builder()
            .label(&info.abbrev)
            .valign(gtk::Align::Center)
            .css_classes(["section-badge"])
            .build(),
    );
    if !info.built_in {
        // Translators: {name} is a translation name.
        let label = gettext("Remove {name}").replace("{name}", &info.name);
        let remove = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text(gettext("Remove"))
            .valign(gtk::Align::Center)
            .action_name("win.remove-translation")
            .action_target(&info.id.to_variant())
            .css_classes(["flat", "error"])
            .build();
        remove.update_property(&[gtk::accessible::Property::Label(&label)]);
        row.add_suffix(&remove);
    }
    row
}

impl SelahWindow {
    pub(super) fn setup_translation_actions(&self) {
        self.setup_catalog_actions();
        self.add_action_entries([
            gio::ActionEntry::builder("translations")
                .activate(|win: &Self, _, _| win.show_translations_dialog())
                .build(),
            gio::ActionEntry::builder("import-translation")
                .activate(|win: &Self, _, _| win.choose_package())
                .build(),
            gio::ActionEntry::builder("remove-translation")
                .parameter_type(Some(glib::VariantTy::STRING))
                .activate(|win: &Self, _, param| {
                    if let Some(id) = param.and_then(|p| p.str()) {
                        win.confirm_remove(id);
                    }
                })
                .build(),
            // Opens a canonical verse in another translation (Saved page).
            gio::ActionEntry::builder("open-in-translation")
                .parameter_type(Some(&<(String, String)>::static_variant_type()))
                .activate(|win: &Self, _, param| {
                    if let Some((id, key)) = param.and_then(|p| p.get::<(String, String)>()) {
                        win.open_translation(&id, Some(Ref::parse(&key)));
                    }
                })
                .build(),
        ]);
    }

    fn show_translations_dialog(&self) {
        let builder =
            gtk::Builder::from_resource("/io/github/cjts15/Selah/ui/translations-dialog.ui");
        let expect = "defined in translations-dialog.blp";
        let dialog: adw::Dialog = builder.object("dialog").expect(expect);
        let language_filter: gtk::DropDown = builder.object("language_filter").expect(expect);
        dialog.connect_closed(clone!(
            #[weak(rename_to = win)]
            self,
            move |_| {
                win.imp().translations_dialog.take();
            }
        ));
        *self.imp().translations_dialog.borrow_mut() = Some(TranslationsDialog {
            dialog: dialog.clone(),
            list: builder.object("installed_list").expect(expect),
            toasts: builder.object("toasts").expect(expect),
            available_stack: builder.object("available_stack").expect(expect),
            available_list: builder.object("available_list").expect(expect),
            available_message: builder.object("available_message").expect(expect),
            language_filter: language_filter.clone(),
            available_rows: RefCell::default(),
        });
        language_filter.connect_selected_notify(clone!(
            #[weak(rename_to = win)]
            self,
            move |_| win.refresh_available()
        ));
        self.refresh_translations_dialog();
        dialog.present(Some(self));
        self.catalog_needed();
    }

    pub(super) fn refresh_translations_dialog(&self) {
        let imp = self.imp();
        let dialog = imp.translations_dialog.borrow();
        let Some(dialog) = dialog.as_ref() else {
            return;
        };
        dialog.list.remove_all();
        for info in imp.translations.borrow().iter() {
            dialog.list.append(&translation_row(info));
        }
        // Installed versions decide what the Available group offers.
        self.refresh_available();
    }

    /// Shows a toast in the Translations dialog if it is open, else in the window.
    fn translations_toast(&self, toast: adw::Toast) {
        match self.imp().translations_dialog.borrow().as_ref() {
            Some(dialog) => dialog.toasts.add_toast(toast),
            None => self.imp().toast_overlay.add_toast(toast),
        }
    }

    /// The Translations dialog if open, else the window, as a parent for alerts.
    fn alert_parent(&self) -> gtk::Widget {
        match self.imp().translations_dialog.borrow().as_ref() {
            Some(dialog) => dialog.dialog.clone().upcast(),
            None => self.clone().upcast(),
        }
    }

    fn choose_package(&self) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&gettext("Selah Translations")));
        filter.add_suffix("selah");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        // Inside Flatpak this is the file chooser portal; no file access is needed.
        let chooser = gtk::FileDialog::builder()
            .title(gettext("Import Translation"))
            .modal(true)
            .filters(&filters)
            .default_filter(&filter)
            .build();
        chooser.open(
            Some(self),
            gio::Cancellable::NONE,
            clone!(
                #[weak(rename_to = win)]
                self,
                move |result| match result {
                    Ok(file) => win.install(file),
                    Err(err) if err.matches(gtk::DialogError::Dismissed) => {}
                    Err(err) => glib::g_warning!("selah", "File chooser: {err}"),
                }
            ),
        );
    }

    fn install(&self, file: gio::File) {
        let built_in: Vec<(String, String)> = self
            .imp()
            .translations
            .borrow()
            .iter()
            .filter(|t| t.built_in)
            .map(|t| (t.id.clone(), t.name.clone()))
            .collect();
        glib::spawn_future_local(clone!(
            #[weak(rename_to = win)]
            self,
            async move {
                let task = move || install_package(&file, &installed_dir(), &built_in);
                match gio::spawn_blocking(task).await {
                    Ok(Ok(installed)) => win.installed(&installed),
                    Ok(Err(err)) => win.import_failed(&err),
                    Err(panic) => std::panic::resume_unwind(panic),
                }
            }
        ));
    }

    pub(super) fn installed(&self, installed: &Installed) {
        // An update of the open translation: show the new text.
        if installed.replaced && *self.imp().translation_id.borrow() == installed.id {
            self.open_translation(&installed.id, Some(self.canonical_ref(None)));
        }
        let title = if installed.replaced {
            // Translators: {name} is a translation name.
            gettext("Updated {name}")
        } else {
            // Translators: {name} is a translation name.
            gettext("{name} installed")
        }
        .replace("{name}", &installed.name);
        let toast = adw::Toast::builder()
            .title(title)
            .use_markup(false)
            .button_label(gettext("Open"))
            .action_name("win.translation")
            .action_target(&installed.id.to_variant())
            .build();
        self.translations_toast(toast);
        self.reload_translations();
    }

    fn import_failed(&self, err: &ImportError) {
        glib::g_warning!("selah", "Import failed: {err:?}");
        let alert = adw::AlertDialog::new(
            Some(&gettext("Could Not Import Translation")),
            Some(&err.message()),
        );
        alert.add_response("close", &gettext("_Close"));
        alert.present(Some(&self.alert_parent()));
    }

    fn confirm_remove(&self, id: &str) {
        let Some(info) = self
            .imp()
            .translations
            .borrow()
            .iter()
            .find(|t| t.id == id && !t.built_in)
            .cloned()
        else {
            return;
        };
        // Translators: {name} is a translation name.
        let heading = gettext("Remove {name}?").replace("{name}", &info.name);
        let alert = adw::AlertDialog::new(
            Some(&heading),
            Some(&gettext("Highlights and bookmarks are kept.")),
        );
        alert.add_responses(&[
            ("cancel", &gettext("_Cancel")),
            ("remove", &gettext("_Remove")),
        ]);
        alert.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
        alert.set_default_response(Some("cancel"));
        alert.set_close_response("cancel");
        alert.connect_response(
            None,
            clone!(
                #[weak(rename_to = win)]
                self,
                move |_, response| {
                    if response == "remove" {
                        win.remove_translation(&info);
                    }
                }
            ),
        );
        alert.present(Some(&self.alert_parent()));
    }

    fn remove_translation(&self, info: &TranslationInfo) {
        // Leave the translation first; the place is kept as a canonical reference.
        if *self.imp().translation_id.borrow() == info.id {
            self.open_translation(DEFAULT_TRANSLATION, Some(self.canonical_ref(None)));
        }
        let title = match fs::remove_file(&info.path) {
            // Translators: {name} is a translation name.
            Ok(()) => gettext("Removed {name}"),
            Err(err) => {
                glib::g_warning!("selah", "Could not remove {}: {err}", info.path.display());
                // Translators: {name} is a translation name.
                gettext("Could not remove {name}")
            }
        }
        .replace("{name}", &info.name);
        self.translations_toast(plain_toast(&title));
        self.reload_translations();
    }
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};

    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("selah-app-{}-{name}", std::process::id()));
        let _ = fs::remove_file(&path);
        path
    }

    /// Gzips `input` bytes with the system gzip.
    fn gzip(input: &[u8]) -> Vec<u8> {
        let mut child = Command::new("gzip")
            .arg("-c")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let input = input.to_vec();
        let writer = std::thread::spawn(move || stdin.write_all(&input));
        let output = child.wait_with_output().unwrap();
        writer.join().unwrap().unwrap();
        output.stdout
    }

    fn unpack_bytes(name: &str, package: &[u8]) -> Result<Vec<u8>, ImportError> {
        let (input, out) = (temp_path(&format!("{name}.selah")), temp_path(name));
        fs::write(&input, package).unwrap();
        let result = unpack(&gio::File::for_path(&input), &out).map(|()| fs::read(&out).unwrap());
        let _ = fs::remove_file(input);
        let _ = fs::remove_file(out);
        result
    }

    #[test]
    fn unpacks_within_limits() {
        let data = b"SQLite format 3\0 and more".repeat(1000);
        assert_eq!(unpack_bytes("ok", &gzip(&data)).unwrap(), data);

        assert!(matches!(
            unpack_bytes("plain", b"not gzip at all"),
            Err(ImportError::NotPackage)
        ));
        let packed = gzip(&data);
        assert!(matches!(
            unpack_bytes("cut", &packed[..packed.len() / 2]),
            Err(ImportError::NotPackage)
        ));
        let bomb = gzip(&vec![0; usize::try_from(MAX_UNPACKED).unwrap() + 1]);
        assert!(bomb.len() < 1024 * 1024, "compresses well");
        assert!(matches!(
            unpack_bytes("bomb", &bomb),
            Err(ImportError::TooLarge)
        ));
    }

    /// A gzip package of a one-verse translation with the given id.
    fn package(id: &str) -> Vec<u8> {
        package_with(id, None)
    }

    /// Like [`package`], after running `sql` on the database with the sqlite3 tool.
    fn package_with(id: &str, sql: Option<&str>) -> Vec<u8> {
        use selah_core::import::{parse_meta, parse_usfm};

        let db = temp_path(&format!("{id}-source.sqlite"));
        let meta = parse_meta(&format!(
            "id={id}\nname=Test {id}\nlanguage=en\ndirection=ltr\nlicence=PD\n"
        ))
        .unwrap();
        let books = vec![parse_usfm("\\id JHN\n\\c 3\n\\v 16 For God so loved.\n").unwrap()];
        store::create(&db, &meta, &books, &Vec::new()).unwrap();
        if let Some(sql) = sql {
            let status = Command::new("sqlite3")
                .arg(&db)
                .arg(sql)
                .output()
                .unwrap()
                .status;
            assert!(status.success());
        }
        let bytes = gzip(&fs::read(&db).unwrap());
        let _ = fs::remove_file(db);
        bytes
    }

    fn install_bytes(dir: &Path, name: &str, bytes: &[u8]) -> Result<Installed, ImportError> {
        let input = temp_path(&format!("{name}.selah"));
        fs::write(&input, bytes).unwrap();
        let built_in = [("eng-kjv".to_owned(), "King James Version".to_owned())];
        let result = install_package(&gio::File::for_path(&input), dir, &built_in);
        let _ = fs::remove_file(input);
        result
    }

    #[test]
    fn installs_replaces_and_refuses() {
        let dir = std::env::temp_dir().join(format!("selah-app-{}-installed", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let files = || {
            let mut names: Vec<String> = fs::read_dir(&dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect();
            names.sort();
            names
        };

        let installed = install_bytes(&dir, "first", &package("test-one")).unwrap();
        assert_eq!(
            (installed.id.as_str(), installed.replaced),
            ("test-one", false)
        );
        assert_eq!(installed.name, "Test test-one");
        assert!(Translation::open(&dir.join("test-one.sqlite")).is_ok());
        assert!(
            install_bytes(&dir, "again", &package("test-one"))
                .unwrap()
                .replaced
        );
        // A WAL database must not leave -wal or -shm files behind.
        let wal = package_with("test-one", Some("PRAGMA journal_mode=WAL;"));
        assert!(install_bytes(&dir, "wal", &wal).unwrap().replaced);

        assert!(matches!(
            install_bytes(&dir, "builtin", &package("eng-kjv")),
            Err(ImportError::BuiltIn(name)) if name == "King James Version"
        ));
        assert!(matches!(
            install_bytes(&dir, "garbage", &gzip(b"not a database")),
            Err(ImportError::NotPackage)
        ));
        // Only the installed file remains: no temporary or refused files.
        assert_eq!(files(), vec!["test-one.sqlite".to_owned()]);
        let _ = fs::remove_dir_all(&dir);
    }
}
