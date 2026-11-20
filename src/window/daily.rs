//! The verse of the day card above the book list.

use selah_core::daily;
use selah_core::user::Ref;

use super::*;

impl SelahWindow {
    pub(super) fn setup_daily_verse(&self) {
        self.add_action_entries([gio::ActionEntry::builder("open-daily-verse")
            .activate(|win: &Self, _, _| {
                let verse = win.imp().daily_verse.borrow().clone();
                if let Some(Ref {
                    book,
                    chapter,
                    verse: Some(verse),
                }) = verse
                {
                    win.open_chapter_by_id(&book, chapter, Some((verse, verse)));
                }
            })
            .build()]);
        self.settings().connect_changed(
            Some("daily-verse"),
            glib::clone!(
                #[weak(rename_to = win)]
                self,
                move |_, _| win.update_daily_visibility()
            ),
        );
        self.schedule_midnight();
    }

    /// Loads today's verse from the open translation (after a switch, too).
    pub(super) fn load_daily_verse(&self) {
        let path = self.translation_path();
        let day = glib::DateTime::now_local().map_or(1, |now| now.day_of_year());
        let day = u32::try_from(day).unwrap_or(1);
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = win)]
            self,
            async move {
                let found = gio::spawn_blocking(move || {
                    let translation = Translation::open(&path).ok()?;
                    // The day's verse, or the next day's if this translation lacks it.
                    daily::for_day(day).find_map(|canonical| {
                        let at = translation.from_canonical(&canonical).ok()??;
                        let text = translation
                            .verse_text(&at.book, at.chapter, at.verse?)
                            .ok()??;
                        Some((at, text))
                    })
                })
                .await
                .ok()
                .flatten();
                win.show_daily_verse(found);
            }
        ));
    }

    fn show_daily_verse(&self, found: Option<(Ref, String)>) {
        let imp = self.imp();
        let shown = found.and_then(|(at, text)| {
            let name = imp
                .books
                .borrow()
                .iter()
                .find(|b| b.id == at.book)?
                .name
                .clone();
            let title = reference_title(&name, at.chapter, at.verse.map(|v| (v, v)));
            Some((at, title, text))
        });
        match shown {
            Some((at, title, text)) => {
                // Translation text is plain text; labels do not parse markup by default.
                imp.daily_reference.set_label(&title);
                imp.daily_text.set_label(&text);
                // Translators: accessible label of the verse of the day card.
                let label = gettext("Verse of the Day, {reference}: {text}")
                    .replace("{reference}", &title)
                    .replace("{text}", &text);
                imp.daily_card
                    .update_property(&[gtk::accessible::Property::Label(&label)]);
                *imp.daily_verse.borrow_mut() = Some(at);
            }
            None => *imp.daily_verse.borrow_mut() = None,
        }
        self.update_daily_visibility();
    }

    /// Shown first in the sidebar, in both modes, when enabled and loaded.
    pub(super) fn update_daily_visibility(&self) {
        let imp = self.imp();
        let visible = self.settings().boolean("daily-verse") && imp.daily_verse.borrow().is_some();
        imp.daily_card.set_visible(visible);
    }

    /// Loads the new day's verse just after local midnight, then waits again.
    fn schedule_midnight(&self) {
        let Ok(now) = glib::DateTime::now_local() else {
            return;
        };
        let Some(next) = now.add_days(1).ok().and_then(|t| {
            glib::DateTime::from_local(t.year(), t.month(), t.day_of_month(), 0, 0, 0.0).ok()
        }) else {
            return;
        };
        let seconds = u32::try_from(next.difference(&now).as_seconds()).unwrap_or(60) + 1;
        glib::timeout_add_seconds_local_once(
            seconds,
            glib::clone!(
                #[weak(rename_to = win)]
                self,
                move || {
                    win.load_daily_verse();
                    win.schedule_midnight();
                }
            ),
        );
    }
}
