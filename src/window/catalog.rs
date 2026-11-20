//! The Available group of the Translations dialog: the online catalogue and
//! downloads. Nothing touches the network before the dialog opens.

use std::collections::HashMap;

use selah_core::catalog::{self, Entry};
use soup::prelude::*;

use super::translations::{ImportError, Installed, install_package};
use super::*;

const CATALOG_URL: &str = "https://cjts15.github.io/selah-catalog/catalog.json";
/// The catalogue is a small JSON file; anything larger is not it.
const MAX_CATALOG: usize = 1024 * 1024;

#[derive(Debug)]
enum Download {
    /// Bytes received so far.
    Running(u64),
    Failed(String),
}

#[derive(Debug, Default)]
pub struct CatalogState {
    entries: Option<Vec<Entry>>,
    error: Option<String>,
    loading: bool,
    downloads: HashMap<String, Download>,
    tasks: HashMap<String, glib::JoinHandle<()>>,
}

impl SelahWindow {
    pub(super) fn setup_catalog_actions(&self) {
        self.add_action_entries([
            gio::ActionEntry::builder("refresh-catalog")
                .activate(|win: &Self, _, _| win.fetch_catalog())
                .build(),
            gio::ActionEntry::builder("download-translation")
                .parameter_type(Some(glib::VariantTy::STRING))
                .activate(|win: &Self, _, param| {
                    if let Some(id) = param.and_then(|p| p.str()) {
                        win.download(id);
                    }
                })
                .build(),
            gio::ActionEntry::builder("cancel-download")
                .parameter_type(Some(glib::VariantTy::STRING))
                .activate(|win: &Self, _, param| {
                    if let Some(id) = param.and_then(|p| p.str()) {
                        win.cancel_download(id);
                    }
                })
                .build(),
        ]);
    }

    fn http(&self) -> &soup::Session {
        self.imp().http.get_or_init(|| {
            let session = soup::Session::new();
            // A generic client name: no user or system details.
            session.set_user_agent(&format!("Selah/{}", config::VERSION));
            session.set_timeout(60);
            session
        })
    }

    /// Fetches the catalogue the first time the dialog opens, or after a failure.
    pub(super) fn catalog_needed(&self) {
        let state = self.imp().catalog.borrow();
        let needed = state.entries.is_none() && !state.loading;
        drop(state);
        if needed {
            self.fetch_catalog();
        }
    }

    fn fetch_catalog(&self) {
        {
            let mut state = self.imp().catalog.borrow_mut();
            if state.loading {
                return;
            }
            state.loading = true;
        }
        self.refresh_available();
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = win)]
            self,
            async move {
                let result = win.request_catalog().await;
                let mut state = win.imp().catalog.borrow_mut();
                state.loading = false;
                match result {
                    Ok(entries) => {
                        state.entries = Some(entries);
                        state.error = None;
                    }
                    Err(err) => {
                        glib::g_warning!("selah", "Catalogue: {err}");
                        state.error = Some(err);
                    }
                }
                drop(state);
                win.refresh_available();
            }
        ));
    }

    async fn request_catalog(&self) -> Result<Vec<Entry>, String> {
        let message = soup::Message::new("GET", CATALOG_URL).map_err(|err| err.to_string())?;
        let bytes = self
            .http()
            .send_and_read_future(&message, glib::Priority::DEFAULT)
            .await
            .map_err(|err| err.to_string())?;
        if message.status_code() != 200 {
            return Err(format!("HTTP {}", message.status_code()));
        }
        if bytes.len() > MAX_CATALOG {
            return Err("catalogue too large".into());
        }
        catalog::parse(&bytes).map_err(|err| err.to_string())
    }

    /// Catalogue entries to offer: not built in, not installed at this version
    /// or newer. The flag is true for an update of an installed translation.
    fn offered(&self) -> Vec<(Entry, bool)> {
        let translations = self.imp().translations.borrow();
        let state = self.imp().catalog.borrow();
        let Some(entries) = &state.entries else {
            return Vec::new();
        };
        entries
            .iter()
            .filter_map(
                |entry| match translations.iter().find(|t| t.id == entry.id) {
                    None => Some((entry.clone(), false)),
                    Some(t) if !t.built_in && t.version < entry.version => {
                        Some((entry.clone(), true))
                    }
                    Some(_) => None,
                },
            )
            .collect()
    }

    /// Redraws the Available group from the catalogue and download state.
    pub(super) fn refresh_available(&self) {
        let imp = self.imp();
        let dialog = imp.translations_dialog.borrow();
        let Some(dialog) = dialog.as_ref() else {
            return;
        };
        let offered = self.offered();
        let state = imp.catalog.borrow();

        // Language filter: "All Languages", then the languages on offer.
        let mut languages: Vec<String> = offered.iter().map(|(e, _)| language_of(e)).collect();
        languages.sort();
        languages.dedup();
        let model = dialog
            .language_filter
            .model()
            .and_downcast::<gtk::StringList>();
        let current: Vec<String> = model
            .as_ref()
            .map(|m| {
                (1..m.n_items())
                    .filter_map(|i| m.string(i))
                    .map(Into::into)
                    .collect()
            })
            .unwrap_or_default();
        let chosen = match dialog.language_filter.selected() {
            0 | gtk::INVALID_LIST_POSITION => None,
            i => current.get(i as usize - 1).cloned(),
        };
        if current != languages {
            let all = gettext("All Languages");
            let mut items = vec![all.as_str()];
            items.extend(languages.iter().map(String::as_str));
            // Setting the model resets the selection, which redraws again; harmless.
            dialog
                .language_filter
                .set_model(Some(&gtk::StringList::new(&items)));
            let position = chosen
                .as_ref()
                .and_then(|c| languages.iter().position(|l| l == c))
                .map_or(0, |i| i as u32 + 1);
            dialog.language_filter.set_selected(position);
        }
        dialog.language_filter.set_visible(languages.len() > 1);

        let shown: Vec<&(Entry, bool)> = offered
            .iter()
            .filter(|(e, _)| chosen.as_ref().is_none_or(|c| language_of(e) == *c))
            .collect();
        let message = if state.loading && state.entries.is_none() {
            dialog.available_stack.set_visible_child_name("loading");
            return;
        } else if let (Some(_), None) = (&state.error, &state.entries) {
            Some(gettext(
                "Could not load the catalogue. Check your connection, then refresh.",
            ))
        } else if shown.is_empty() {
            Some(gettext("Every translation in the catalogue is installed."))
        } else {
            None
        };
        if let Some(message) = message {
            dialog.available_message.set_label(&message);
            dialog.available_stack.set_visible_child_name("message");
            return;
        }

        dialog.available_list.remove_all();
        let mut rows = dialog.available_rows.borrow_mut();
        rows.clear();
        for (entry, update) in shown {
            // Catalogue data is plain text, never markup.
            let row = adw::ActionRow::builder()
                .title(&entry.name)
                .use_markup(false)
                .build();
            if !entry.abbreviation.is_empty() {
                row.add_suffix(
                    &gtk::Label::builder()
                        .label(&entry.abbreviation)
                        .valign(gtk::Align::Center)
                        .css_classes(["section-badge"])
                        .build(),
                );
            }
            let button = gtk::Button::builder()
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();
            row.add_suffix(&button);
            update_row(
                &row,
                &button,
                entry,
                *update,
                state.downloads.get(&entry.id),
            );
            dialog.available_list.append(&row);
            rows.insert(entry.id.clone(), (row, button));
        }
        dialog.available_stack.set_visible_child_name("list");
    }

    /// Updates one Available row without rebuilding the list, keeping focus.
    fn refresh_available_row(&self, id: &str) {
        let imp = self.imp();
        let dialog = imp.translations_dialog.borrow();
        let Some(dialog) = dialog.as_ref() else {
            return;
        };
        let rows = dialog.available_rows.borrow();
        let Some((row, button)) = rows.get(id) else {
            return;
        };
        let offered = self.offered();
        let Some((entry, update)) = offered.iter().find(|(e, _)| e.id == id) else {
            return;
        };
        update_row(
            row,
            button,
            entry,
            *update,
            imp.catalog.borrow().downloads.get(id),
        );
    }

    fn download(&self, id: &str) {
        let Some((entry, _)) = self.offered().into_iter().find(|(e, _)| e.id == id) else {
            return;
        };
        {
            let mut state = self.imp().catalog.borrow_mut();
            if matches!(state.downloads.get(id), Some(Download::Running(_))) {
                return;
            }
            state.downloads.insert(id.to_owned(), Download::Running(0));
        }
        self.refresh_available_row(id);

        let built_in: Vec<(String, String)> = self
            .imp()
            .translations
            .borrow()
            .iter()
            .filter(|t| t.built_in)
            .map(|t| (t.id.clone(), t.name.clone()))
            .collect();
        let task = glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = win)]
            self,
            async move {
                let id = entry.id.clone();
                let result = match win.fetch_package(&entry).await {
                    Ok(data) => gio::spawn_blocking(move || {
                        verify_and_install(&entry, &data, &installed_dir(), &built_in)
                    })
                    .await
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
                    Err(err) => Err(err),
                };
                win.imp().catalog.borrow_mut().tasks.remove(&id);
                match result {
                    Ok(installed) => {
                        win.imp().catalog.borrow_mut().downloads.remove(&id);
                        win.installed(&installed);
                    }
                    Err(err) => {
                        glib::g_warning!("selah", "Download of {id} failed: {err}");
                        win.imp()
                            .catalog
                            .borrow_mut()
                            .downloads
                            .insert(id.clone(), Download::Failed(err));
                        win.refresh_available_row(&id);
                    }
                }
            }
        ));
        self.imp()
            .catalog
            .borrow_mut()
            .tasks
            .insert(id.to_owned(), task);
    }

    /// Downloads a package into memory (at most its catalogue size), reporting progress.
    async fn fetch_package(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let message = soup::Message::new("GET", &entry.url).map_err(|err| err.to_string())?;
        let stream = self
            .http()
            .send_future(&message, glib::Priority::DEFAULT)
            .await
            .map_err(|err| err.to_string())?;
        if message.status_code() != 200 {
            return Err(format!("HTTP {}", message.status_code()));
        }
        let expected = usize::try_from(entry.size).map_err(|err| err.to_string())?;
        let mut data = Vec::with_capacity(expected);
        loop {
            let bytes = stream
                .read_bytes_future(64 * 1024, glib::Priority::DEFAULT)
                .await
                .map_err(|err| err.to_string())?;
            if bytes.is_empty() {
                break;
            }
            data.extend_from_slice(&bytes);
            if data.len() > expected {
                return Err(gettext("size did not match"));
            }
            self.imp()
                .catalog
                .borrow_mut()
                .downloads
                .insert(entry.id.clone(), Download::Running(data.len() as u64));
            self.refresh_available_row(&entry.id);
        }
        if data.len() != expected {
            return Err(gettext("size did not match"));
        }
        Ok(data)
    }

    fn cancel_download(&self, id: &str) {
        let task = {
            let mut state = self.imp().catalog.borrow_mut();
            state.downloads.remove(id);
            state.tasks.remove(id)
        };
        // Dropping the future cancels its network request.
        if let Some(task) = task {
            task.abort();
        }
        self.refresh_available_row(id);
    }
}

fn language_of(entry: &Entry) -> String {
    if entry.language_name.is_empty() {
        entry.language.clone()
    } else {
        entry.language_name.clone()
    }
}

/// Sets a row's subtitle and button for its download state.
fn update_row(
    row: &adw::ActionRow,
    button: &gtk::Button,
    entry: &Entry,
    update: bool,
    download: Option<&Download>,
) {
    let size = glib::format_size(entry.size).to_string();
    let (subtitle, icon, tooltip, action) = match download {
        Some(Download::Running(received)) => (
            // Translators: download progress, e.g. "Downloading 3.2 MB of 5.0 MB".
            gettext("Downloading {received} of {total}")
                .replace("{received}", &glib::format_size(*received))
                .replace("{total}", &size),
            "process-stop-symbolic",
            gettext("Cancel Download"),
            "win.cancel-download",
        ),
        Some(Download::Failed(reason)) => (
            // Translators: {reason} explains the failure, e.g. "checksum did not match".
            gettext("Download failed: {reason}").replace("{reason}", reason),
            "view-refresh-symbolic",
            gettext("Retry"),
            "win.download-translation",
        ),
        None => (
            [language_of(entry), entry.licence.clone(), size]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · "),
            if update {
                "software-update-available-symbolic"
            } else {
                "folder-download-symbolic"
            },
            if update {
                gettext("Update")
            } else {
                gettext("Download")
            },
            "win.download-translation",
        ),
    };
    row.set_subtitle(&subtitle);
    button.set_icon_name(icon);
    button.set_tooltip_text(Some(&tooltip));
    // Translators: accessible label of a catalogue button, e.g. "Download Reina-Valera 1909".
    let label = gettext("{action} {name}")
        .replace("{action}", &tooltip)
        .replace("{name}", &entry.name);
    button.update_property(&[gtk::accessible::Property::Label(&label)]);
    button.set_action_name(Some(action));
    button.set_action_target_value(Some(&entry.id.to_variant()));
}

/// Checks the sha256 the catalogue promised, then installs the package in `dir`.
fn verify_and_install(
    entry: &Entry,
    data: &[u8],
    dir: &std::path::Path,
    built_in: &[(String, String)],
) -> Result<Installed, String> {
    let mut checksum = glib::Checksum::new(glib::ChecksumType::Sha256)
        .ok_or_else(|| "sha256 unavailable".to_owned())?;
    checksum.update(data);
    if checksum.string().as_deref() != Some(entry.sha256.as_str()) {
        return Err(gettext("checksum did not match"));
    }
    std::fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    // Not a .sqlite name, so a leftover is never listed.
    let path = dir.join(format!(
        ".download-{}-{}.selah",
        entry.id,
        std::process::id()
    ));
    std::fs::write(&path, data).map_err(|err| err.to_string())?;
    let result = install_package(&gio::File::for_path(&path), dir, built_in);
    let _ = std::fs::remove_file(&path);
    result.map_err(|err: ImportError| err.message())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_a_wrong_checksum() {
        let dir = std::env::temp_dir().join(format!("selah-app-{}-download", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let data = b"not even a package".to_vec();
        let mut entry = Entry {
            id: "test-dl".into(),
            name: "Test".into(),
            abbreviation: String::new(),
            language: "en".into(),
            language_name: String::new(),
            direction: "ltr".into(),
            licence: "PD".into(),
            version: "2026-11-20".into(),
            url: "https://example.org/test-dl.selah".into(),
            size: data.len() as u64,
            sha256: "00".repeat(32),
        };
        let err = verify_and_install(&entry, &data, &dir, &[]).unwrap_err();
        assert_eq!(err, gettext("checksum did not match"));

        // The right checksum gets as far as unpacking, which refuses this data.
        let mut checksum = glib::Checksum::new(glib::ChecksumType::Sha256).unwrap();
        checksum.update(&data);
        entry.sha256 = checksum.string().unwrap();
        let err = verify_and_install(&entry, &data, &dir, &[]).unwrap_err();
        assert_ne!(err, gettext("checksum did not match"));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "no files left");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
