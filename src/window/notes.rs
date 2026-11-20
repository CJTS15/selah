//! Notes: one plain-text note per verse, written in a small dialog.

use std::collections::{HashMap, HashSet};

use selah_core::user::Ref;

use super::*;

impl SelahWindow {
    pub(super) fn setup_notes(&self) {
        self.add_action_entries([gio::ActionEntry::builder("edit-note")
            .activate(|win: &Self, _, _| win.edit_note_for_current())
            .build()]);
    }

    /// Verses of the open chapter (as this translation numbers them) with a note.
    pub(super) fn note_verses(&self, content: &Chapter) -> HashSet<u32> {
        // Canonical (book, chapter) to look up, and canonical verse to the verse shown.
        let mut shown: HashMap<(&str, u32, u32), u32> = HashMap::new();
        for (verse, canonical) in content.verses.iter().zip(&content.canonical) {
            if let Some(Ref {
                book,
                chapter,
                verse: Some(number),
            }) = canonical
            {
                shown.insert((book, *chapter, *number), verse.verse);
            }
        }
        let mut chapters: Vec<(&str, u32)> = shown.keys().map(|(b, c, _)| (*b, *c)).collect();
        chapters.sort_unstable();
        chapters.dedup();
        let mut verses = HashSet::new();
        for (book, chapter) in chapters {
            for number in self
                .with_db(|db| db.verse_notes(book, chapter))
                .unwrap_or_default()
            {
                if let Some(verse) = shown.get(&(book, chapter, number)) {
                    verses.insert(*verse);
                }
            }
        }
        verses
    }

    /// Ctrl+N and "Add Note…": the tapped verse, or the verse at the caret.
    fn edit_note_for_current(&self) {
        let imp = self.imp();
        let verse = self.active_verse().or_else(|| {
            let buffer = imp.text_view.buffer();
            let offset = buffer.iter_at_mark(&buffer.get_insert()).offset();
            imp.verse_ranges
                .borrow()
                .iter()
                .find(|(_, start, end)| (*start..=*end).contains(&offset))
                .map(|(verse, _, _)| *verse)
        });
        if let Some(verse) = verse {
            self.close_verse_actions();
            self.open_note_dialog(verse);
        }
    }

    pub(super) fn open_note_dialog(&self, verse: u32) {
        // Verses without a canonical reference (2 Esdras 7:36–105 in the WEB) cannot be saved.
        let (Some(reference), Some(title)) =
            (self.canonical_ref(Some(verse)), self.verse_title(verse))
        else {
            return;
        };
        let existing = self.with_db(|db| db.note_of(&reference)).flatten();

        let builder = gtk::Builder::from_resource("/io/github/cjts15/Selah/ui/note-dialog.ui");
        let expect = "defined in note-dialog.blp";
        let dialog: adw::Dialog = builder.object("dialog").expect(expect);
        let text: gtk::TextView = builder.object("note_text").expect(expect);
        let save: gtk::Button = builder.object("save_button").expect(expect);
        let cancel: gtk::Button = builder.object("cancel_button").expect(expect);
        let delete: gtk::Button = builder.object("delete_button").expect(expect);

        // Translators: title of the note dialog; {reference} is a verse, such as "John 3:16".
        dialog.set_title(&gettext("Note on {reference}").replace("{reference}", &title));
        text.buffer()
            .set_text(existing.as_deref().unwrap_or_default());
        delete.set_visible(existing.is_some());
        dialog.set_focus(Some(&text));

        cancel.connect_clicked(glib::clone!(
            #[weak]
            dialog,
            move |_| {
                dialog.close();
            }
        ));
        save.connect_clicked(glib::clone!(
            #[weak(rename_to = win)]
            self,
            #[weak]
            dialog,
            #[weak]
            text,
            #[strong]
            reference,
            move |_| {
                let buffer = text.buffer();
                let (start, end) = buffer.bounds();
                let body = buffer.text(&start, &end, false);
                if win.with_db(|db| db.set_note(&reference, &body)).is_some() {
                    win.after_note_change();
                }
                dialog.close();
            }
        ));
        delete.connect_clicked(glib::clone!(
            #[weak(rename_to = win)]
            self,
            #[weak]
            dialog,
            move |_| {
                if win.with_db(|db| db.set_note(&reference, "")).is_some() {
                    win.after_note_change();
                    if let Some(old) = existing.clone() {
                        win.note_deleted_toast(reference.clone(), old);
                    }
                }
                dialog.close();
            }
        ));
        // Ctrl+Enter saves, as in other GNOME text dialogs.
        let shortcuts = gtk::ShortcutController::new();
        shortcuts.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("<Control>Return"),
            Some(gtk::CallbackAction::new(glib::clone!(
                #[weak]
                save,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, _| {
                    save.emit_clicked();
                    glib::Propagation::Stop
                }
            ))),
        ));
        dialog.add_controller(shortcuts);
        dialog.present(Some(self));
    }

    fn note_deleted_toast(&self, reference: Ref, body: String) {
        let toast = adw::Toast::builder()
            .title(gettext("Note deleted"))
            .button_label(gettext("Undo"))
            .build();
        toast.connect_button_clicked(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |_| {
                if win.with_db(|db| db.set_note(&reference, &body)).is_some() {
                    win.after_note_change();
                }
            }
        ));
        self.imp().toast_overlay.add_toast(toast);
    }

    /// Notes change the reader's text (the note marks), so redraw it.
    pub(super) fn after_note_change(&self) {
        self.rerender_chapter();
        self.refresh_saved();
    }
}
