//! The Saved page: highlights and bookmarks, with filters in the sidebar.

use selah_core::user::{Colour, Kind, Ref};

use super::verse::colour_name;
use super::*;

/// Where a saved item is in the open translation.
#[derive(Debug)]
struct Shown {
    /// The reference as this translation numbers it, if it has it.
    at: Option<Ref>,
    /// The verse text, for verse items.
    text: Option<String>,
    /// Another translation (id, name) that has it, when this one does not.
    elsewhere: Option<(String, String)>,
}

/// Sidebar filter rows: All, Bookmarks, then one per colour.
fn filter_at(index: i32) -> Option<Filter> {
    match index {
        0 => Some(Filter::All),
        1 => Some(Filter::Bookmarks),
        i => usize::try_from(i - 2)
            .ok()
            .and_then(|i| Colour::ALL.get(i))
            .map(|&c| Filter::Colour(c)),
    }
}

fn filter_index(filter: Filter) -> i32 {
    match filter {
        Filter::All => 0,
        Filter::Bookmarks => 1,
        Filter::Colour(colour) => {
            2 + Colour::ALL
                .iter()
                .position(|&c| c == colour)
                .and_then(|i| i32::try_from(i).ok())
                .unwrap_or(0)
        }
    }
}

fn colour_dot(colour: Colour) -> gtk::Box {
    gtk::Box::builder()
        .valign(gtk::Align::Center)
        .css_classes(["colour-dot", colour.as_str()])
        .build()
}

impl SelahWindow {
    pub(super) fn setup_saved(&self) {
        let imp = self.imp();
        // Built here: Blueprint's StringList items are deprecated in GTK 4.24.
        imp.saved_order.set_model(Some(&gtk::StringList::new(&[
            &gettext("Recent"),
            &gettext("Bible Order"),
        ])));
        imp.sidebar_mode.connect_active_name_notify(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |_| win.sidebar_mode_changed()
        ));
        imp.saved_filters.connect_row_activated(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |_, row| win.select_filter(row.index())
        ));
        imp.saved_filters.set_header_func(|row, _| {
            if row.index() == 2 {
                let header = gtk::Label::builder()
                    .label(gettext("Highlights"))
                    .xalign(0.0)
                    .margin_top(12)
                    .margin_bottom(3)
                    .margin_start(12)
                    .css_classes(["caption-heading", "dimmed"])
                    .build();
                row.set_header(Some(&header));
            } else {
                row.set_header(None::<&gtk::Widget>);
            }
        });
        imp.saved_order.connect_selected_notify(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |_| win.show_saved()
        ));
        self.add_action_entries([gio::ActionEntry::builder("remove-annotation")
            .parameter_type(Some(glib::VariantTy::INT64))
            .activate(|win: &Self, _, param| {
                if let Some(id) = param.and_then(|p| p.get::<i64>()) {
                    win.remove_annotation(id);
                }
            })
            .build()]);
    }

    fn sidebar_mode_changed(&self) {
        let imp = self.imp();
        let saved = imp.sidebar_mode.active_name().as_deref() == Some("saved");
        imp.sidebar_stack
            .set_visible_child_name(if saved { "saved" } else { "books" });
        imp.book_filter.set_visible(!saved);
        if saved {
            self.refresh_filters();
            imp.content_stack.set_visible_child_name("saved");
            self.show_saved();
        } else {
            imp.content_stack.set_visible_child_name("reading");
        }
    }

    fn select_filter(&self, index: i32) {
        let imp = self.imp();
        imp.saved_filter.set(filter_at(index));
        imp.content_stack.set_visible_child_name("saved");
        imp.split_view.set_show_content(true);
        self.show_saved();
    }

    /// Rebuilds the filter rows with fresh counts, keeping the selection.
    fn refresh_filters(&self) {
        let imp = self.imp();
        let list = &imp.saved_filters;
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }
        let (bookmarks, colours) = self.with_db(|db| db.counts()).unwrap_or_default();
        let total = bookmarks + colours.iter().map(|(_, n)| n).sum::<usize>();

        let mut rows: Vec<(gtk::Widget, String, usize)> = vec![
            (
                gtk::Image::from_icon_name("view-list-symbolic").upcast(),
                gettext("All"),
                total,
            ),
            (
                gtk::Image::from_icon_name("user-bookmarks-symbolic").upcast(),
                gettext("Bookmarks"),
                bookmarks,
            ),
        ];
        for (colour, count) in colours {
            rows.push((colour_dot(colour).upcast(), colour_name(colour), count));
        }
        for (icon, name, count) in rows {
            let content = gtk::Box::builder().spacing(12).build();
            content.append(&icon);
            content.append(
                &gtk::Label::builder()
                    .label(&name)
                    .xalign(0.0)
                    .hexpand(true)
                    .build(),
            );
            content.append(
                &gtk::Label::builder()
                    .label(count.to_string())
                    .css_classes(["caption", "dimmed", "numeric"])
                    .build(),
            );
            let row = gtk::ListBoxRow::builder().child(&content).build();
            // Translators: accessible label of a Saved filter, e.g. "Yellow, 3".
            let label = gettext("{name}, {count}")
                .replace("{name}", &name)
                .replace("{count}", &count.to_string());
            row.update_property(&[gtk::accessible::Property::Label(&label)]);
            list.append(&row);
        }
        let selected = filter_index(imp.saved_filter.get().unwrap_or(Filter::All));
        list.select_row(list.row_at_index(selected).as_ref());
    }

    /// Updates the Saved filters and page after highlights or bookmarks change.
    pub(super) fn refresh_saved(&self) {
        let imp = self.imp();
        if imp.sidebar_mode.active_name().as_deref() == Some("saved") {
            self.refresh_filters();
        }
        if imp.content_stack.visible_child_name().as_deref() == Some("saved") {
            self.show_saved();
        }
    }

    fn show_saved(&self) {
        let imp = self.imp();
        let filter = imp.saved_filter.get().unwrap_or(Filter::All);
        let title = match filter {
            Filter::All => gettext("All Saved"),
            Filter::Bookmarks => gettext("Bookmarks"),
            // Translators: {colour} is a highlight colour name such as "Yellow".
            Filter::Colour(colour) => {
                gettext("{colour} Highlights").replace("{colour}", &colour_name(colour))
            }
        };
        imp.saved_title.set_title(&title);
        // Translators: {translation} is a Bible translation name.
        imp.saved_title.set_subtitle(
            &gettext("Shown in {translation}")
                .replace("{translation}", &imp.translation_name.borrow()),
        );

        let mut items = self.with_db(|db| db.list(filter)).unwrap_or_default();
        let bible_order = imp.saved_order.selected() == 1;
        if bible_order {
            let books = imp.books.borrow();
            items.sort_by_key(|a| {
                let book = books
                    .iter()
                    .position(|b| b.id == a.reference.book)
                    .unwrap_or(usize::MAX);
                (book, a.reference.chapter, a.reference.verse.unwrap_or(0))
            });
        }
        *imp.saved_items.borrow_mut() = items.clone();

        let generation = imp.saved_generation.get() + 1;
        imp.saved_generation.set(generation);
        let path = self.translation_path();
        let refs: Vec<_> = items.iter().map(|a| a.reference.clone()).collect();
        // Other translations, to offer one that has a verse this one lacks.
        let current = imp.translation_id.borrow().clone();
        let others: Vec<(String, String, PathBuf)> = imp
            .translations
            .borrow()
            .iter()
            .filter(|t| t.id != current)
            .map(|t| (t.id.clone(), t.name.clone(), t.path.clone()))
            .collect();
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = win)]
            self,
            async move {
                let result = gio::spawn_blocking(move || {
                    let translation = Translation::open(&path)?;
                    let mut opened: Vec<Option<Translation>> =
                        others.iter().map(|_| None).collect();
                    // Where each saved reference is in this translation, with its text.
                    refs.iter()
                        .map(|r| {
                            let shown = translation.from_canonical(r)?;
                            let text = match &shown {
                                Some(Ref {
                                    book,
                                    chapter,
                                    verse: Some(verse),
                                }) => translation.verse_text(book, *chapter, *verse)?,
                                _ => None,
                            };
                            let mut elsewhere = None;
                            if shown.is_none() {
                                for ((id, name, path), other) in others.iter().zip(&mut opened) {
                                    // A damaged file only loses the offer.
                                    if other.is_none() {
                                        *other = Translation::open(path).ok();
                                    }
                                    let has = other
                                        .as_ref()
                                        .and_then(|t| t.from_canonical(r).ok().flatten())
                                        .is_some();
                                    if has {
                                        elsewhere = Some((id.clone(), name.clone()));
                                        break;
                                    }
                                }
                            }
                            Ok::<_, selah_core::Error>(Shown {
                                at: shown,
                                text,
                                elsewhere,
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .await;
                if win.imp().saved_generation.get() != generation {
                    return;
                }
                match result {
                    Ok(Ok(shown)) => win.render_saved(&items, &shown, bible_order),
                    Ok(Err(err)) => win.show_error(&err.to_string()),
                    Err(panic) => std::panic::resume_unwind(panic),
                }
            }
        ));
    }

    fn render_saved(&self, items: &[Annotation], shown: &[Shown], bible_order: bool) {
        let imp = self.imp();
        let page = &imp.saved_list;
        while let Some(child) = page.first_child() {
            page.remove(&child);
        }
        imp.saved_stack
            .set_visible_child_name(if items.is_empty() { "empty" } else { "list" });

        let books = imp.books.borrow();
        let translation = imp.translation_name.borrow();
        let today = glib::DateTime::now_local().ok().map(|d| d.ymd());
        let mut current_group: Option<bool> = None;
        let mut list: Option<gtk::ListBox> = None;

        for (item, shown) in items.iter().zip(shown) {
            let (native, text) = (&shown.at, &shown.text);
            // Recent order groups by day: Today, then Earlier.
            let is_today = glib::DateTime::from_unix_local(item.modified)
                .ok()
                .map(|d| d.ymd())
                == today;
            if list.is_none() || (!bible_order && current_group != Some(is_today)) {
                if !bible_order {
                    let header = gtk::Label::builder()
                        .label(if is_today {
                            gettext("Today")
                        } else {
                            gettext("Earlier")
                        })
                        .xalign(0.0)
                        .margin_top(12)
                        .margin_bottom(6)
                        .css_classes(["heading"])
                        .build();
                    page.append(&header);
                }
                let new_list = gtk::ListBox::builder()
                    .selection_mode(gtk::SelectionMode::None)
                    .css_classes(["boxed-list"])
                    .build();
                page.append(&new_list);
                list = Some(new_list);
                current_group = Some(is_today);
            }

            // Shown as this translation numbers it; the stored reference if absent.
            let present = native.is_some();
            let r = native.as_ref().unwrap_or(&item.reference);
            let book = books.iter().find(|b| b.id == r.book);
            let name = book.map_or(r.book.as_str(), |b| b.name.as_str());

            let content = gtk::Box::builder()
                .spacing(12)
                .margin_top(9)
                .margin_bottom(9)
                .margin_start(12)
                .margin_end(6)
                .build();
            let prefix: gtk::Widget = match (item.kind, item.colour) {
                (Kind::Highlight, Some(colour)) => colour_dot(colour).upcast(),
                _ => gtk::Image::builder()
                    .icon_name("user-bookmarks-symbolic")
                    .valign(gtk::Align::Center)
                    .css_classes(["accent"])
                    .build()
                    .upcast(),
            };
            content.append(&prefix);
            let labels = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(3)
                .hexpand(true)
                .build();
            labels.append(
                &gtk::Label::builder()
                    .label(reference_title(name, r.chapter, r.verse.map(|v| (v, v))))
                    .xalign(0.0)
                    .css_classes(["heading"])
                    .build(),
            );
            let body = match (text, present) {
                (Some(text), _) => gtk::Label::builder()
                    .label(text)
                    .wrap(true)
                    .wrap_mode(pango::WrapMode::WordChar)
                    .xalign(0.0)
                    .css_classes(["search-snippet"])
                    .build(),
                (None, true) => gtk::Label::builder()
                    .label(gettext("Chapter bookmark"))
                    .xalign(0.0)
                    .css_classes(["dimmed", "caption"])
                    .build(),
                (None, false) => {
                    // Translators: {translation} is a Bible translation name.
                    let mut label =
                        gettext("Not in {translation}").replace("{translation}", &translation);
                    if let Some((_, other)) = &shown.elsewhere {
                        // Translators: follows "Not in …"; {translation} is a Bible translation name.
                        label = format!(
                            "{label}. {}",
                            gettext("Open in {translation}").replace("{translation}", other)
                        );
                    }
                    gtk::Label::builder()
                        .label(label)
                        .wrap(true)
                        .xalign(0.0)
                        .css_classes(["dimmed", "caption"])
                        .build()
                }
            };
            labels.append(&body);
            content.append(&labels);

            let menu = gio::Menu::new();
            let remove = gio::MenuItem::new(Some(&gettext("Remove")), None);
            remove.set_action_and_target_value(
                Some("win.remove-annotation"),
                Some(&item.id.to_variant()),
            );
            menu.append_item(&remove);
            let more = gtk::MenuButton::builder()
                .icon_name("view-more-symbolic")
                .menu_model(&menu)
                .valign(gtk::Align::Center)
                .tooltip_text(gettext("More Options"))
                .css_classes(["flat"])
                .build();
            more.update_property(&[gtk::accessible::Property::Label(&gettext("More Options"))]);
            content.append(&more);

            let row = gtk::ListBoxRow::builder()
                .child(&content)
                .activatable(present || shown.elsewhere.is_some())
                .build();
            if let Some((id, _)) = shown.elsewhere.as_ref().filter(|_| !present) {
                row.set_action_name(Some("win.open-in-translation"));
                row.set_action_target_value(Some(
                    &(id.as_str(), item.reference.key().as_str()).to_variant(),
                ));
            }
            if present {
                match r.verse {
                    Some(verse) => {
                        row.set_action_name(Some("win.open-verse"));
                        row.set_action_target_value(Some(
                            &(r.book.as_str(), r.chapter, verse, verse).to_variant(),
                        ));
                    }
                    None => {
                        row.set_action_name(Some("win.open-chapter"));
                        row.set_action_target_value(Some(
                            &(r.book.as_str(), r.chapter).to_variant(),
                        ));
                    }
                }
            }
            if let Some(list) = &list {
                list.append(&row);
            }
        }
    }

    fn remove_annotation(&self, id: i64) {
        let Some(item) = self
            .imp()
            .saved_items
            .borrow()
            .iter()
            .find(|a| a.id == id)
            .cloned()
        else {
            return;
        };
        if self.with_db(|db| db.delete(id)).is_none() {
            return;
        }
        let toast = adw::Toast::builder()
            .title(gettext("Removed"))
            .button_label(gettext("Undo"))
            .build();
        toast.connect_button_clicked(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |_| {
                win.with_db(|db| db.restore(&item));
                win.after_annotation_change();
            }
        ));
        self.imp().toast_overlay.add_toast(toast);
        self.after_annotation_change();
    }
}
