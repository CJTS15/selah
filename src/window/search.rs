//! The search page: reference jumps, full-text results and recent searches.

use selah_core::refs::{self, Reference};
use selah_core::store::{MATCH_END, MATCH_START, Scope, SearchOptions, SearchResults};

use super::*;

const RESULT_LIMIT: usize = 200;
const RECENT_LIMIT: usize = 8;

/// Pango markup for a snippet: text escaped, matches bold on a translucent yellow.
fn snippet_markup(snippet: &str) -> String {
    let mut markup = String::new();
    let mut open = false;
    for (i, part) in snippet.split([MATCH_START, MATCH_END]).enumerate() {
        if i > 0 {
            // Each marker toggles the match; the split keeps them in order.
            markup.push_str(if open {
                "</b></span>"
            } else {
                "<span background=\"#f6d32d\" bgalpha=\"45%\"><b>"
            });
            open = !open;
        }
        markup.push_str(&glib::markup_escape_text(part));
    }
    if open {
        markup.push_str("</b></span>");
    }
    markup
}

fn section_label(section: Section) -> String {
    match section {
        Section::OldTestament => gettext("Old Testament"),
        Section::NewTestament => gettext("New Testament"),
        Section::Deuterocanon => gettext("Deuterocanon"),
    }
}

fn group_header(title: &str, detail: Option<&str>) -> gtk::Box {
    let header = gtk::Box::builder()
        .spacing(6)
        .margin_top(12)
        .margin_bottom(6)
        .build();
    header.append(
        &gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .css_classes(["heading"])
            .build(),
    );
    if let Some(detail) = detail {
        header.append(
            &gtk::Label::builder()
                .label(detail)
                .xalign(0.0)
                .css_classes(["dimmed"])
                .build(),
        );
    }
    header
}

fn boxed_list() -> gtk::ListBox {
    gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build()
}

/// A row that opens a verse range when activated.
fn verse_row(book: &str, chapter: u32, verses: (u32, u32)) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::builder().build();
    row.set_action_name(Some("win.open-verse"));
    row.set_action_target_value(Some(&(book, chapter, verses.0, verses.1).to_variant()));
    row
}

impl SelahWindow {
    pub(super) fn setup_search(&self) {
        let imp = self.imp();
        imp.search_entry.connect_search_changed(clone!(
            #[weak(rename_to = win)]
            self,
            move |_| win.run_search()
        ));
        imp.search_entry.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_| {
                if let Some(row) = win.imp().first_result.borrow().clone() {
                    row.activate();
                }
            }
        ));
        imp.search_entry.connect_stop_search(clone!(
            #[weak(rename_to = win)]
            self,
            move |_| win.close_search()
        ));
        imp.scope_group.connect_active_name_notify(clone!(
            #[weak(rename_to = win)]
            self,
            move |_| win.run_search()
        ));

        // Down moves from the entry into the results.
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(clone!(
            #[weak(rename_to = win)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, _| {
                if key == gtk::gdk::Key::Down
                    && let Some(row) = win.imp().first_result.borrow().as_ref()
                {
                    row.grab_focus();
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            }
        ));
        imp.search_entry.add_controller(keys);
    }

    pub(super) fn open_search(&self) {
        let imp = self.imp();
        let book_name = imp
            .current
            .get()
            .and_then(|(book, _)| imp.books.borrow().get(book).map(|b| b.name.clone()));
        imp.book_toggle.set_label(book_name.as_deref());
        imp.book_toggle.set_enabled(book_name.is_some());
        if imp.scope_group.active_name().as_deref() == Some("book") && book_name.is_none() {
            imp.scope_group.set_active_name(Some("all"));
        }

        imp.content_stack.set_visible_child_name("search");
        imp.split_view.set_show_content(true);
        imp.search_entry.grab_focus();
        imp.search_entry.select_region(0, -1);
        self.run_search();
    }

    pub(super) fn close_search(&self) {
        let imp = self.imp();
        imp.content_stack.set_visible_child_name("reading");
        imp.text_view.grab_focus();
    }

    fn scope(&self) -> Scope {
        let imp = self.imp();
        match imp.scope_group.active_name().as_deref() {
            Some("ot") => Scope::Section(Section::OldTestament),
            Some("nt") => Scope::Section(Section::NewTestament),
            Some("book") => imp
                .current
                .get()
                .and_then(|(book, _)| imp.books.borrow().get(book).map(|b| b.id.clone()))
                .map_or(Scope::All, Scope::Book),
            _ => Scope::All,
        }
    }

    pub(super) fn run_search(&self) {
        let imp = self.imp();
        let generation = imp.search_generation.get() + 1;
        imp.search_generation.set(generation);

        let text = imp.search_entry.text().trim().to_owned();
        imp.scope_bar.set_visible(!text.is_empty());
        if text.is_empty() {
            self.show_recent();
            return;
        }

        let reference = refs::parse_reference(&text, &imp.books.borrow());
        let preview_target = reference.and_then(|r| {
            let books = imp.books.borrow();
            Some((books.get(r.book)?.id.clone(), r.chapter?, r.verses?.0))
        });
        let scope = self.scope();
        let options = SearchOptions {
            whole_words: self.settings().boolean("search-whole-words"),
            hide_deuterocanon: !self.settings().boolean("show-deuterocanon"),
        };
        let path = self.translation_path();
        glib::spawn_future_local(clone!(
            #[weak(rename_to = win)]
            self,
            async move {
                let result = gio::spawn_blocking(move || {
                    let translation = Translation::open(&path)?;
                    let preview = match preview_target {
                        Some((book, chapter, verse)) => translation
                            .chapter(&book, chapter)?
                            .verses
                            .into_iter()
                            .find(|v| v.verse == verse)
                            .map(|v| v.text),
                        None => None,
                    };
                    let results = translation.search(&text, &scope, RESULT_LIMIT, options)?;
                    Ok::<_, selah_core::Error>((preview, results))
                })
                .await;
                // Typing continued while this search ran.
                if win.imp().search_generation.get() != generation {
                    return;
                }
                match result {
                    Ok(Ok((preview, results))) => {
                        win.show_search(reference, preview.as_deref(), &results);
                    }
                    Ok(Err(err)) => win.show_error(&err.to_string()),
                    Err(panic) => std::panic::resume_unwind(panic),
                }
            }
        ));
    }

    fn clear_results(&self) {
        let imp = self.imp();
        while let Some(child) = imp.search_results.first_child() {
            imp.search_results.remove(&child);
        }
        imp.first_result.replace(None);
        imp.search_stack.set_visible_child_name("results");
    }

    /// Notes the first activatable row, which Enter and Down use.
    fn note_first(&self, row: &gtk::ListBoxRow) {
        let first = &self.imp().first_result;
        if first.borrow().is_none() {
            first.replace(Some(row.clone()));
        }
    }

    fn show_search(
        &self,
        reference: Option<Reference>,
        preview: Option<&str>,
        results: &SearchResults,
    ) {
        let imp = self.imp();
        self.clear_results();
        let books = imp.books.borrow();

        if let Some(reference) = reference
            && let Some(book) = books.get(reference.book)
        {
            let chapter = reference.chapter.or_else(|| book.chapters.first().copied());
            if let Some(chapter) = chapter {
                imp.search_results
                    .append(&group_header(&gettext("Go to"), None));
                let list = boxed_list();
                // A verse only gets a row when it exists; its text is the proof.
                if let (Some(verses), Some(preview)) = (reference.verses, preview) {
                    let row = verse_row(&book.id, chapter, verses);
                    row.set_child(Some(&self.go_to_content(
                        &reference_title(&book.name, chapter, Some(verses)),
                        preview,
                        true,
                    )));
                    list.append(&row);
                    self.note_first(&row);
                }
                let row = gtk::ListBoxRow::builder().build();
                row.set_action_name(Some("win.open-chapter"));
                row.set_action_target_value(Some(&(book.id.as_str(), chapter).to_variant()));
                row.set_child(Some(&self.go_to_content(
                    &reference_title(&book.name, chapter, None),
                    &gettext("Whole chapter"),
                    imp.first_result.borrow().is_none(),
                )));
                list.append(&row);
                self.note_first(&row);
                imp.search_results.append(&list);
            }
        }

        let total = results.total();
        if total == 0 {
            if reference.is_none() {
                imp.search_stack.set_visible_child_name("empty");
            }
            return;
        }

        for &(section, count) in &results.counts {
            let count_text = ngettext(
                // Translators: number of search results in a section, e.g. "12 verses".
                "{n} verse",
                "{n} verses",
                u32::try_from(count).unwrap_or(u32::MAX),
            )
            .replace("{n}", &count.to_string());
            imp.search_results
                .append(&group_header(&section_label(section), Some(&count_text)));
            let list = boxed_list();
            for hit in &results.hits {
                let Some(book) = books.iter().find(|b| b.id == hit.book) else {
                    continue;
                };
                if book.section != section {
                    continue;
                }
                let row = verse_row(&hit.book, hit.chapter, (hit.verse, hit.verse));
                let content = gtk::Box::builder()
                    .orientation(gtk::Orientation::Vertical)
                    .spacing(3)
                    .margin_top(9)
                    .margin_bottom(9)
                    .margin_start(12)
                    .margin_end(12)
                    .build();
                content.append(
                    &gtk::Label::builder()
                        .label(reference_title(
                            &book.name,
                            hit.chapter,
                            Some((hit.verse, hit.verse)),
                        ))
                        .xalign(0.0)
                        .css_classes(["heading"])
                        .build(),
                );
                content.append(
                    &gtk::Label::builder()
                        .label(snippet_markup(&hit.snippet))
                        .use_markup(true)
                        .wrap(true)
                        .wrap_mode(pango::WrapMode::WordChar)
                        .xalign(0.0)
                        .css_classes(["search-snippet"])
                        .build(),
                );
                row.set_child(Some(&content));
                list.append(&row);
                self.note_first(&row);
            }
            if list.first_child().is_some() {
                imp.search_results.append(&list);
            }
        }

        if results.hits.len() < total {
            // Translators: {shown} and {total} are numbers of verses.
            let note = gettext("Showing the first {shown} of {total} verses")
                .replace("{shown}", &results.hits.len().to_string())
                .replace("{total}", &total.to_string());
            imp.search_results.append(
                &gtk::Label::builder()
                    .label(note)
                    .margin_top(12)
                    .css_classes(["dimmed"])
                    .build(),
            );
        }
    }

    fn go_to_content(&self, title: &str, subtitle: &str, primary: bool) -> gtk::Box {
        let content = gtk::Box::builder()
            .spacing(12)
            .margin_top(9)
            .margin_bottom(9)
            .margin_start(12)
            .margin_end(12)
            .build();
        content.append(&gtk::Image::from_icon_name("go-next-symbolic"));
        let labels = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .build();
        labels.append(
            &gtk::Label::builder()
                .label(title)
                .xalign(0.0)
                .css_classes(["heading"])
                .build(),
        );
        labels.append(
            &gtk::Label::builder()
                .label(subtitle)
                .xalign(0.0)
                .ellipsize(pango::EllipsizeMode::End)
                .css_classes(["dimmed"])
                .build(),
        );
        content.append(&labels);
        if primary {
            content.append(
                &gtk::Label::builder()
                    // Translators: the Enter key, shown next to the default search result.
                    .label(gettext("Enter"))
                    .valign(gtk::Align::Center)
                    .css_classes(["dimmed", "caption"])
                    .build(),
            );
        }
        content
    }

    fn show_recent(&self) {
        let imp = self.imp();
        self.clear_results();
        let recent = self.settings().strv("recent-searches");

        if !recent.is_empty() {
            let header = group_header(&gettext("Recent"), None);
            let clear = gtk::Button::builder()
                .label(gettext("Clear"))
                .halign(gtk::Align::End)
                .hexpand(true)
                .css_classes(["flat"])
                .build();
            clear.connect_clicked(clone!(
                #[weak(rename_to = win)]
                self,
                move |_| {
                    if let Err(err) = win.settings().set_strv("recent-searches", &[] as &[&str]) {
                        glib::g_warning!("selah", "Failed to clear recent searches: {err}");
                    }
                    win.show_recent();
                }
            ));
            header.append(&clear);
            imp.search_results.append(&header);

            let list = boxed_list();
            for text in recent.iter() {
                let content = gtk::Box::builder()
                    .spacing(12)
                    .margin_top(12)
                    .margin_bottom(12)
                    .margin_start(12)
                    .margin_end(12)
                    .build();
                content.append(&gtk::Image::from_icon_name("document-open-recent-symbolic"));
                content.append(
                    &gtk::Label::builder()
                        .label(text.as_str())
                        .xalign(0.0)
                        .build(),
                );
                let row = gtk::ListBoxRow::builder().child(&content).build();
                list.append(&row);
                self.note_first(&row);
            }
            list.connect_row_activated(clone!(
                #[weak(rename_to = win)]
                self,
                move |_, row| {
                    let recent = win.settings().strv("recent-searches");
                    if let Some(text) = usize::try_from(row.index())
                        .ok()
                        .and_then(|i| recent.get(i))
                    {
                        win.imp().search_entry.set_text(text.as_str());
                        win.imp().search_entry.set_position(-1);
                    }
                }
            ));
            imp.search_results.append(&list);
        }

        // Translators: {translation} is a Bible translation name.
        let hint = gettext("Type a reference such as “Gen 1:1” or “Ps 23”, or any words to search the {translation}.")
            .replace("{translation}", &imp.translation_name.borrow());
        imp.search_results.append(
            &gtk::Label::builder()
                .label(hint)
                .wrap(true)
                .justify(gtk::Justification::Center)
                .margin_top(18)
                .css_classes(["dimmed"])
                .build(),
        );
    }

    /// Remembers the entry text as a recent search, newest first.
    pub(super) fn remember_search(&self) {
        let text = self.imp().search_entry.text().trim().to_owned();
        if text.is_empty() {
            return;
        }
        let mut recent: Vec<String> = self
            .settings()
            .strv("recent-searches")
            .iter()
            .map(|s| s.to_string())
            .filter(|s| s != &text)
            .collect();
        recent.insert(0, text);
        recent.truncate(RECENT_LIMIT);
        if let Err(err) = self.settings().set_strv("recent-searches", recent) {
            glib::g_warning!("selah", "Failed to save recent searches: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_markup_escapes_and_marks() {
        assert_eq!(
            snippet_markup("a <b> & \u{1}life\u{2}."),
            "a &lt;b&gt; &amp; <span background=\"#f6d32d\" bgalpha=\"45%\"><b>life</b></span>."
        );
        // An unclosed match is closed so the markup stays valid.
        assert!(snippet_markup("\u{1}x").ends_with("</b></span>"));
        assert_eq!(snippet_markup("plain"), "plain");
    }

    #[test]
    fn titles() {
        assert_eq!(reference_title("John", 3, None), "John 3");
        assert_eq!(reference_title("John", 3, Some((16, 16))), "John 3:16");
        assert_eq!(reference_title("Psalms", 23, Some((1, 4))), "Psalms 23:1–4");
    }
}
