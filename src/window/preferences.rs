//! The Preferences dialog, and the reader following preference changes.

use selah_core::user::Ref;

use super::*;

/// Settings values for the combo rows, in row order (default first).
const FONTS: &[&str] = &["serif", "sans"];
const SPACINGS: &[&str] = &["comfortable", "compact", "spacious"];
const LAYOUTS: &[&str] = &["paragraphs", "lines"];
const SPEEDS: &[&str] = &["normal", "slower", "faster"];

/// Binds a string setting with fixed `values` to a combo row showing `labels`.
fn bind_choice(
    settings: &gio::Settings,
    key: &str,
    row: &adw::ComboRow,
    values: &'static [&'static str],
    labels: &[String],
) {
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    row.set_model(Some(&gtk::StringList::new(&labels)));
    settings
        .bind(key, row, "selected")
        .mapping(move |variant, _| {
            let value: String = variant.get()?;
            let index = values.iter().position(|v| *v == value).unwrap_or(0);
            Some(u32::try_from(index).unwrap_or(0).to_value())
        })
        .set_mapping(move |value, _| {
            let index: u32 = value.get().ok()?;
            values.get(index as usize).map(|v| v.to_variant())
        })
        .build();
}

impl SelahWindow {
    pub(super) fn setup_preferences(&self) {
        self.add_action_entries([gio::ActionEntry::builder("preferences")
            .activate(|win: &Self, _, _| win.show_preferences())
            .build()]);

        let settings = self.settings();
        for key in ["font", "line-spacing"] {
            settings.connect_changed(
                Some(key),
                glib::clone!(
                    #[weak(rename_to = win)]
                    self,
                    move |_, _| win.apply_text_style()
                ),
            );
        }
        for key in ["verse-layout", "verse-numbers"] {
            settings.connect_changed(
                Some(key),
                glib::clone!(
                    #[weak(rename_to = win)]
                    self,
                    move |_, _| win.rerender_chapter()
                ),
            );
        }
        // Reloading the translation re-filters its books, at the same place.
        settings.connect_changed(
            Some("show-deuterocanon"),
            glib::clone!(
                #[weak(rename_to = win)]
                self,
                move |_, _| {
                    let id = win.imp().translation_id.borrow().clone();
                    win.open_translation(&id, Some(win.canonical_ref(None)));
                }
            ),
        );
        settings.connect_changed(
            Some("search-whole-words"),
            glib::clone!(
                #[weak(rename_to = win)]
                self,
                move |_, _| {
                    if win.imp().content_stack.visible_child_name().as_deref() == Some("search") {
                        win.run_search();
                    }
                }
            ),
        );
    }

    /// Draws the open chapter again, keeping the verse at the top in view.
    pub(super) fn rerender_chapter(&self) {
        let imp = self.imp();
        let Some((index, chapter)) = imp.current.get() else {
            return;
        };
        let Some(book) = imp.books.borrow().get(index).cloned() else {
            return;
        };
        let Some(content) = imp.chapter_content.borrow().clone() else {
            return;
        };
        let text_view = &imp.text_view;
        let rect = text_view.visible_rect();
        let top = text_view
            .iter_at_location(rect.x(), rect.y())
            .and_then(|iter| {
                let offset = iter.offset();
                imp.verse_ranges
                    .borrow()
                    .iter()
                    .find(|(_, _, end)| *end >= offset)
                    .map(|(verse, _, _)| *verse)
            });
        imp.keep_in_view.set(top);
        self.show_chapter(&book, chapter, &content, None);
    }

    fn show_preferences(&self) {
        let builder =
            gtk::Builder::from_resource("/io/github/cjts15/Selah/ui/preferences-dialog.ui");
        let expect = "defined in preferences-dialog.blp";
        let dialog: adw::PreferencesDialog = builder.object("dialog").expect(expect);
        let settings = self.settings();

        bind_choice(
            settings,
            "font",
            &builder.object("font_row").expect(expect),
            FONTS,
            &[gettext("Serif"), gettext("Sans Serif")],
        );
        bind_choice(
            settings,
            "line-spacing",
            &builder.object("line_spacing_row").expect(expect),
            SPACINGS,
            &[
                gettext("Comfortable"),
                gettext("Compact"),
                gettext("Spacious"),
            ],
        );
        bind_choice(
            settings,
            "verse-layout",
            &builder.object("verse_layout_row").expect(expect),
            LAYOUTS,
            &[gettext("Paragraphs"), gettext("One Verse per Line")],
        );
        bind_choice(
            settings,
            "read-speed",
            &builder.object("read_speed_row").expect(expect),
            SPEEDS,
            &[gettext("Normal"), gettext("Slower"), gettext("Faster")],
        );
        for (key, row) in [
            ("verse-numbers", "verse_numbers_row"),
            ("daily-verse", "daily_verse_row"),
            ("show-deuterocanon", "deuterocanon_row"),
            ("search-whole-words", "whole_words_row"),
        ] {
            let row: adw::SwitchRow = builder.object(row).expect(expect);
            settings.bind(key, &row, "active").build();
        }
        let text_size: gtk::Label = builder.object("text_size").expect(expect);
        settings
            .bind("text-scale", &text_size, "label")
            .get_only()
            .mapping(|variant, _| {
                let scale: f64 = variant.get()?;
                Some(format!("{}%", (scale * 100.0).round()).to_value())
            })
            .build();

        // The preview shares the reader's tags, so font, size and spacing follow.
        let preview: gtk::TextView = builder.object("preview").expect(expect);
        let buffer = gtk::TextBuffer::new(Some(&self.imp().text_view.buffer().tag_table()));
        preview.set_buffer(Some(&buffer));
        self.fill_preview(&preview);
        // The default translation is the one opened at start-up: the picker's key.
        let translation_row: adw::ComboRow = builder.object("translation_row").expect(expect);
        let ids: Vec<String> = self
            .imp()
            .translations
            .borrow()
            .iter()
            .map(|t| t.id.clone())
            .collect();
        let names: Vec<String> = self
            .imp()
            .translations
            .borrow()
            .iter()
            .map(|t| t.name.clone())
            .collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        translation_row.set_model(Some(&gtk::StringList::new(&names)));
        let current = self.imp().translation_id.borrow().clone();
        if let Some(position) = ids.iter().position(|id| *id == current) {
            translation_row.set_selected(u32::try_from(position).unwrap_or(0));
        }
        translation_row.connect_selected_notify(glib::clone!(
            #[weak(rename_to = win)]
            self,
            #[weak]
            preview,
            move |row| {
                if let Some(id) = ids.get(row.selected() as usize) {
                    win.switch_translation(id);
                    // switch_translation sets the new translation's file at once.
                    win.fill_preview(&preview);
                }
            }
        ));

        let numbers_changed = settings.connect_changed(
            Some("verse-numbers"),
            glib::clone!(
                #[weak(rename_to = win)]
                self,
                #[weak]
                preview,
                move |_, _| win.fill_preview(&preview)
            ),
        );
        let numbers_changed = std::cell::Cell::new(Some(numbers_changed));
        dialog.connect_closed(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |_| {
                if let Some(id) = numbers_changed.take() {
                    win.settings().disconnect(id);
                }
            }
        ));
        dialog.present(Some(self));
    }

    /// Psalm 23:1 from the open translation (John 1:1 if it has no Psalms).
    fn fill_preview(&self, preview: &gtk::TextView) {
        let numbers = self.settings().boolean("verse-numbers");
        let path = self.translation_path();
        preview.set_direction(self.text_direction());
        glib::spawn_future_local(glib::clone!(
            #[weak]
            preview,
            async move {
                let text = gio::spawn_blocking(move || {
                    let translation = Translation::open(&path).ok()?;
                    ["PSA.23.1", "JHN.1.1"].into_iter().find_map(|key| {
                        let r = translation.from_canonical(&Ref::parse(key)?).ok()??;
                        let text = translation
                            .verse_text(&r.book, r.chapter, r.verse?)
                            .ok()??;
                        Some((r.verse?, text))
                    })
                })
                .await
                .ok()
                .flatten();
                let Some((verse, text)) = text else {
                    return;
                };
                let buffer = preview.buffer();
                buffer.set_text("");
                let mut end = buffer.end_iter();
                if numbers {
                    buffer.insert_with_tags_by_name(
                        &mut end,
                        &verse.to_string(),
                        &["verse-number"],
                    );
                    buffer.insert(&mut end, "\u{2009}");
                }
                // Translation text is plain text, never markup.
                buffer.insert(&mut end, &text);
                let (start, end) = buffer.bounds();
                buffer.apply_tag_by_name("body", &start, &end);
                // The preview cannot take focus; screen readers get the verse here.
                preview.update_property(&[gtk::accessible::Property::Description(&text)]);
            }
        ));
    }
}
