//! Verse actions in the reader: highlights, bookmarks and copying.

use selah_core::user::{Colour, Ref};

use super::*;

/// Highlight colours: GNOME palette tones, translucent so text stays readable
/// in light and dark styles.
fn colour_rgba(colour: Colour) -> gtk::gdk::RGBA {
    let (r, g, b): (u8, u8, u8) = match colour {
        Colour::Yellow => (0xf6, 0xd3, 0x2d),
        Colour::Green => (0x57, 0xe3, 0x89),
        Colour::Blue => (0x62, 0xa0, 0xea),
        Colour::Pink => (0xed, 0x5b, 0x9a),
        Colour::Purple => (0xc0, 0x61, 0xcb),
    };
    gtk::gdk::RGBA::new(
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        0.4,
    )
}

pub(super) fn colour_name(colour: Colour) -> String {
    match colour {
        Colour::Yellow => gettext("Yellow"),
        Colour::Green => gettext("Green"),
        Colour::Blue => gettext("Blue"),
        Colour::Pink => gettext("Pink"),
        Colour::Purple => gettext("Purple"),
    }
}

impl SelahWindow {
    pub(super) fn open_user_db(&self) {
        let dir = glib::user_data_dir().join("selah");
        let result = std::fs::create_dir_all(&dir)
            .map_err(selah_core::Error::from)
            .and_then(|()| UserDb::open(&dir.join("user.sqlite")));
        match result {
            Ok(db) => *self.imp().user_db.borrow_mut() = Some(db),
            Err(err) => {
                glib::g_warning!("selah", "Could not open saved verses: {err}");
                self.toast(&gettext("Saved verses are unavailable"));
            }
        }
    }

    /// Runs `f` on the user database, reporting failures as a toast.
    pub(super) fn with_db<T>(
        &self,
        f: impl FnOnce(&mut UserDb) -> Result<T, selah_core::Error>,
    ) -> Option<T> {
        let result = f(self.imp().user_db.borrow_mut().as_mut()?);
        match result {
            Ok(value) => Some(value),
            Err(err) => {
                glib::g_warning!("selah", "Saved verses: {err}");
                self.toast(&gettext("Could not save the change"));
                None
            }
        }
    }

    pub(super) fn toast(&self, title: &str) {
        self.imp().toast_overlay.add_toast(adw::Toast::new(title));
    }

    pub(super) fn setup_verse_actions(&self) {
        let imp = self.imp();
        let buffer = imp.text_view.buffer();
        for colour in Colour::ALL {
            buffer.create_tag(
                Some(&format!("hl-{}", colour.as_str())),
                &[("background-rgba", &colour_rgba(colour))],
            );
        }

        // Colour swatches are radio buttons bound to this action's state.
        let highlight = gio::SimpleAction::new_stateful(
            "highlight",
            Some(glib::VariantTy::STRING),
            &"".to_variant(),
        );
        highlight.connect_activate(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |action, param| {
                let Some(colour) = param.and_then(|p| p.str()).and_then(Colour::parse) else {
                    return;
                };
                action.set_state(&colour.as_str().to_variant());
                win.highlight_active(colour);
            }
        ));
        self.add_action(&highlight);

        let bookmark_chapter =
            gio::SimpleAction::new_stateful("bookmark-chapter", None, &false.to_variant());
        bookmark_chapter.connect_activate(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |action, _| {
                let bookmarked = action
                    .state()
                    .and_then(|s| s.get::<bool>())
                    .unwrap_or(false);
                if win.set_chapter_bookmark(!bookmarked) {
                    action.set_state(&(!bookmarked).to_variant());
                }
            }
        ));
        self.add_action(&bookmark_chapter);

        self.add_action_entries([
            gio::ActionEntry::builder("bookmark-verse")
                .activate(|win: &Self, _, _| win.toggle_verse_bookmark())
                .build(),
            gio::ActionEntry::builder("copy-verse")
                .activate(|win: &Self, _, _| win.copy_active_verse())
                .build(),
            gio::ActionEntry::builder("remove-highlight")
                .activate(|win: &Self, _, _| win.remove_active_highlight())
                .build(),
            gio::ActionEntry::builder("caret-browsing")
                .activate(|win: &Self, _, _| {
                    let text_view = &win.imp().text_view;
                    text_view.set_cursor_visible(!text_view.is_cursor_visible());
                })
                .build(),
        ]);

        imp.verse_sheet.connect_open_notify(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |sheet| {
                if !sheet.is_open() {
                    win.clear_verse_selection();
                }
            }
        ));

        // A plain click or tap on a verse opens its actions; drags still select text.
        let click = gtk::GestureClick::new();
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.connect_released(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |_, presses, x, y| {
                if presses == 1 {
                    // Let the text view finish its own click handling first.
                    glib::idle_add_local_once(glib::clone!(
                        #[weak]
                        win,
                        move || win.verse_tapped(x, y)
                    ));
                }
            }
        ));
        imp.text_view.add_controller(click);

        // Shift+F10 or the Menu key opens the actions for the verse at the caret.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = win)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, state| {
                let menu = key == gtk::gdk::Key::Menu
                    || (key == gtk::gdk::Key::F10
                        && state.contains(gtk::gdk::ModifierType::SHIFT_MASK));
                if !menu {
                    return glib::Propagation::Proceed;
                }
                let buffer = win.imp().text_view.buffer();
                let offset = buffer.iter_at_mark(&buffer.get_insert()).offset();
                if win.select_verse_at(offset) {
                    win.show_verse_actions();
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
        ));
        imp.text_view.add_controller(keys);

        // Ctrl+C on a tapped verse copies it with its reference.
        imp.text_view.connect_copy_clipboard(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |text_view| {
                if win.active_verse().is_some() {
                    win.copy_active_verse();
                    text_view.stop_signal_emission_by_name("copy-clipboard");
                }
            }
        ));
    }

    fn verse_range(&self, verse: u32) -> Option<(i32, i32)> {
        self.imp()
            .verse_ranges
            .borrow()
            .iter()
            .find(|(v, _, _)| *v == verse)
            .map(|&(_, start, end)| (start, end))
    }

    fn verse_tapped(&self, x: f64, y: f64) {
        let text_view = &self.imp().text_view;
        if text_view.buffer().has_selection() {
            return;
        }
        // Truncation is fine: buffer coordinates are whole pixels.
        let (bx, by) =
            text_view.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
        if let Some(iter) = text_view.iter_at_location(bx, by)
            && self.select_verse_at(iter.offset())
        {
            self.show_verse_actions();
        }
    }

    /// Selects the whole verse containing `offset` and makes it the active verse.
    fn select_verse_at(&self, offset: i32) -> bool {
        let imp = self.imp();
        let found = imp
            .verse_ranges
            .borrow()
            .iter()
            .find(|(_, start, end)| (*start..=*end).contains(&offset))
            .copied();
        let Some((verse, start, end)) = found else {
            imp.active_verse.set(None);
            return false;
        };
        let buffer = imp.text_view.buffer();
        buffer.select_range(&buffer.iter_at_offset(start), &buffer.iter_at_offset(end));
        imp.active_verse.set(Some(verse));
        true
    }

    /// The active verse, if it is still exactly what is selected.
    fn active_verse(&self) -> Option<u32> {
        let verse = self.imp().active_verse.get()?;
        let (start, end) = self.verse_range(verse)?;
        let (a, b) = self.imp().text_view.buffer().selection_bounds()?;
        (a.offset() == start && b.offset() == end).then_some(verse)
    }

    fn current_ref(&self, verse: Option<u32>) -> Option<Ref> {
        let imp = self.imp();
        let (book, chapter) = imp.current.get()?;
        Some(Ref {
            book: imp.books.borrow().get(book)?.id.clone(),
            chapter,
            verse,
        })
    }

    fn show_verse_actions(&self) {
        let imp = self.imp();
        let (Some(verse), Some(reference)) =
            (self.active_verse(), self.current_ref(self.active_verse()))
        else {
            return;
        };
        let colour = self.with_db(|db| db.highlight_of(&reference)).flatten();
        let bookmarked = self
            .with_db(|db| db.is_bookmarked(&reference))
            .unwrap_or(false);
        if let Some(action) = self.lookup_action("highlight") {
            action.change_state(&colour.map_or("", Colour::as_str).to_variant());
        }
        let Some(book_name) = imp
            .current
            .get()
            .and_then(|(book, _)| imp.books.borrow().get(book).map(|b| b.name.clone()))
        else {
            return;
        };
        let title = reference_title(&book_name, reference.chapter, Some((verse, verse)));

        if imp.split_view.is_collapsed() {
            self.show_verse_sheet(&title, colour.is_some(), bookmarked);
        } else {
            self.show_verse_popover(&title, verse, colour.is_some(), bookmarked);
        }
    }

    fn colour_swatches(&self) -> gtk::Box {
        let swatches = gtk::Box::builder()
            .spacing(12)
            .halign(gtk::Align::Center)
            .margin_top(6)
            .margin_bottom(6)
            .css_classes(["swatches"])
            .build();
        for colour in Colour::ALL {
            let name = colour_name(colour);
            let button = gtk::CheckButton::builder()
                .css_classes([colour.as_str()])
                .tooltip_text(&name)
                .action_name("win.highlight")
                .action_target(&colour.as_str().to_variant())
                .build();
            button.update_property(&[gtk::accessible::Property::Label(&name)]);
            swatches.append(&button);
        }
        swatches
    }

    /// A flat, menu-style button: label on the left, shortcut hint on the right.
    fn menu_button(label: &str, accel: Option<&str>, action: &str) -> gtk::Button {
        let accel = accel
            .and_then(gtk::accelerator_parse)
            .map(|(key, mods)| gtk::accelerator_get_label(key, mods));
        let content = gtk::Box::builder().spacing(24).build();
        content.append(
            &gtk::Label::builder()
                .label(label)
                .xalign(0.0)
                .hexpand(true)
                .build(),
        );
        if let Some(accel) = accel {
            content.append(
                &gtk::Label::builder()
                    .label(accel.as_str())
                    .css_classes(["dimmed"])
                    .build(),
            );
        }
        gtk::Button::builder()
            .child(&content)
            .action_name(action)
            .css_classes(["flat"])
            .build()
    }

    fn show_verse_popover(&self, title: &str, verse: u32, highlighted: bool, bookmarked: bool) {
        let imp = self.imp();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .css_classes(["verse-actions"])
            .build();
        content.append(
            &gtk::Label::builder()
                .label(title)
                .xalign(0.0)
                .margin_start(6)
                .margin_top(3)
                .css_classes(["heading"])
                .build(),
        );
        content.append(&self.colour_swatches());
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let bookmark = if bookmarked {
            gettext("Remove Bookmark")
        } else {
            gettext("Bookmark Verse")
        };
        // Ctrl+C is shown here but handled by the text view, so entries keep their copy.
        content.append(&Self::menu_button(
            &bookmark,
            Some("<Control>d"),
            "win.bookmark-verse",
        ));
        content.append(&Self::menu_button(
            &gettext("Copy with Reference"),
            Some("<Control>c"),
            "win.copy-verse",
        ));
        if highlighted {
            content.append(&Self::menu_button(
                &gettext("Remove Highlight"),
                None,
                "win.remove-highlight",
            ));
        }

        if let Some(old) = imp.verse_popover.take() {
            old.popdown();
            old.unparent();
        }
        let popover = gtk::Popover::builder().child(&content).build();
        // Focus moves into the popover, so Ctrl+C needs its own shortcut here.
        let shortcuts = gtk::ShortcutController::new();
        shortcuts.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("<Control>c"),
            Some(gtk::NamedAction::new("win.copy-verse")),
        ));
        popover.add_controller(shortcuts);
        popover.connect_closed(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |_| win.clear_verse_selection()
        ));
        popover.set_parent(&*imp.text_view);
        if let Some(rect) = self.verse_rect(verse) {
            popover.set_pointing_to(Some(&rect));
        }
        popover.popup();
        imp.verse_popover.replace(Some(popover));
    }

    /// Where the verse starts, in text view coordinates.
    fn verse_rect(&self, verse: u32) -> Option<gtk::gdk::Rectangle> {
        let text_view = &self.imp().text_view;
        let (start, _) = self.verse_range(verse)?;
        let location = text_view.iter_location(&text_view.buffer().iter_at_offset(start));
        let (x, y) = text_view.buffer_to_window_coords(
            gtk::TextWindowType::Widget,
            location.x(),
            location.y(),
        );
        Some(gtk::gdk::Rectangle::new(
            x,
            y,
            location.width().max(1),
            location.height(),
        ))
    }

    fn show_verse_sheet(&self, title: &str, highlighted: bool, bookmarked: bool) {
        let imp = self.imp();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(6)
            .margin_bottom(18)
            .margin_start(18)
            .margin_end(18)
            .build();
        let header = gtk::Box::builder().spacing(6).build();
        header.append(
            &gtk::Label::builder()
                .label(title)
                .xalign(0.0)
                .hexpand(true)
                .css_classes(["heading"])
                .build(),
        );
        let close = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text(gettext("Close"))
            .css_classes(["flat", "circular"])
            .build();
        close.update_property(&[gtk::accessible::Property::Label(&gettext("Close"))]);
        close.connect_clicked(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |_| win.imp().verse_sheet.set_open(false)
        ));
        header.append(&close);
        content.append(&header);
        content.append(&self.colour_swatches());

        let buttons = gtk::Box::builder().spacing(6).homogeneous(true).build();
        let bookmark_label = if bookmarked {
            gettext("Unbookmark")
        } else {
            gettext("Bookmark")
        };
        for (icon, label, action, sensitive) in [
            (
                "user-bookmarks-symbolic",
                bookmark_label,
                "win.bookmark-verse",
                true,
            ),
            (
                "edit-copy-symbolic",
                gettext("Copy"),
                "win.copy-verse",
                true,
            ),
            (
                "user-trash-symbolic",
                gettext("Remove"),
                "win.remove-highlight",
                highlighted,
            ),
        ] {
            let inner = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(6)
                .margin_top(6)
                .margin_bottom(6)
                .build();
            inner.append(&gtk::Image::from_icon_name(icon));
            inner.append(&gtk::Label::new(Some(&label)));
            let button = gtk::Button::builder()
                .child(&inner)
                .action_name(action)
                .sensitive(sensitive)
                .build();
            buttons.append(&button);
        }
        content.append(&buttons);
        imp.verse_sheet.set_sheet(Some(&content));
        imp.verse_sheet.set_open(true);
    }

    fn close_verse_actions(&self) {
        let imp = self.imp();
        if let Some(popover) = imp.verse_popover.borrow().as_ref() {
            popover.popdown();
        }
        imp.verse_sheet.set_open(false);
        self.clear_verse_selection();
    }

    /// Drops the tapped verse's selection so its highlight shows plainly.
    fn clear_verse_selection(&self) {
        if self.active_verse().is_some() {
            let buffer = self.imp().text_view.buffer();
            let insert = buffer.iter_at_mark(&buffer.get_insert());
            buffer.select_range(&insert, &insert);
        }
        self.imp().active_verse.set(None);
    }

    fn highlight_active(&self, colour: Colour) {
        if let Some(reference) = self
            .current_ref(self.active_verse())
            .filter(|r| r.verse.is_some())
        {
            self.with_db(|db| db.set_highlight(&reference, colour));
            self.after_annotation_change();
        }
        self.close_verse_actions();
    }

    fn remove_active_highlight(&self) {
        if let Some(reference) = self
            .current_ref(self.active_verse())
            .filter(|r| r.verse.is_some())
        {
            self.with_db(|db| db.remove_highlight(&reference));
            self.after_annotation_change();
        }
        self.close_verse_actions();
    }

    fn toggle_verse_bookmark(&self) {
        if let Some(reference) = self
            .current_ref(self.active_verse())
            .filter(|r| r.verse.is_some())
        {
            let added = self.with_db(|db| {
                let added = !db.is_bookmarked(&reference)?;
                db.set_bookmark(&reference, added)?;
                Ok(added)
            });
            self.after_annotation_change();
            if let Some(added) = added {
                self.bookmark_toast(&reference, added);
            }
        }
        self.close_verse_actions();
    }

    /// Confirms a bookmark change, naming the verse or chapter.
    fn bookmark_toast(&self, reference: &Ref, added: bool) {
        if !added {
            self.toast(&gettext("Removed Bookmark"));
            return;
        }
        let imp = self.imp();
        let name = imp
            .books
            .borrow()
            .iter()
            .find(|b| b.id == reference.book)
            .map_or_else(|| reference.book.clone(), |b| b.name.clone());
        let title = reference_title(&name, reference.chapter, reference.verse.map(|v| (v, v)));
        // Translators: {reference} is a verse or chapter, such as "John 3:16".
        self.toast(&gettext("Bookmarked {reference}").replace("{reference}", &title));
    }

    fn copy_active_verse(&self) {
        let imp = self.imp();
        let Some(verse) = self.active_verse() else {
            return;
        };
        let text = imp.chapter_content.borrow().as_ref().and_then(|c| {
            c.verses
                .iter()
                .find(|v| v.verse == verse)
                .map(|v| v.text.clone())
        });
        let (Some(text), Some((book, chapter))) = (text, imp.current.get()) else {
            return;
        };
        let Some(book_name) = imp.books.borrow().get(book).map(|b| b.name.clone()) else {
            return;
        };
        // Translators: copied verse; keep the placeholders, adjust the quotation marks.
        let copied = gettext("“{text}” ({reference} {translation})")
            .replace("{text}", &text)
            .replace(
                "{reference}",
                &reference_title(&book_name, chapter, Some((verse, verse))),
            )
            .replace("{translation}", &imp.translation_abbrev.borrow());
        self.clipboard().set_text(&copied);
        self.toast(&gettext("Copied to clipboard"));
        self.close_verse_actions();
    }

    /// Draws the open chapter's highlights.
    pub(super) fn apply_highlights(&self) {
        let imp = self.imp();
        let buffer = imp.text_view.buffer();
        let (start, end) = buffer.bounds();
        for colour in Colour::ALL {
            buffer.remove_tag_by_name(&format!("hl-{}", colour.as_str()), &start, &end);
        }
        let Some(Ref { book, chapter, .. }) = self.current_ref(None) else {
            return;
        };
        for (verse, colour) in self
            .with_db(|db| db.highlights(&book, chapter))
            .unwrap_or_default()
        {
            if let Some((start, end)) = self.verse_range(verse) {
                buffer.apply_tag_by_name(
                    &format!("hl-{}", colour.as_str()),
                    &buffer.iter_at_offset(start),
                    &buffer.iter_at_offset(end),
                );
            }
        }
    }

    pub(super) fn update_chapter_bookmark(&self) {
        let bookmarked = self
            .current_ref(None)
            .and_then(|reference| self.with_db(|db| db.is_bookmarked(&reference)))
            .unwrap_or(false);
        if let Some(action) = self.lookup_action("bookmark-chapter") {
            action.change_state(&bookmarked.to_variant());
        }
    }

    fn set_chapter_bookmark(&self, bookmarked: bool) -> bool {
        let Some(reference) = self.current_ref(None) else {
            return false;
        };
        let done = self
            .with_db(|db| db.set_bookmark(&reference, bookmarked))
            .is_some();
        if done {
            self.refresh_saved();
            self.bookmark_toast(&reference, bookmarked);
        }
        done
    }

    /// Brings the reader and the Saved page up to date after a change.
    pub(super) fn after_annotation_change(&self) {
        self.apply_highlights();
        self.update_chapter_bookmark();
        self.refresh_saved();
    }
}
