use std::cell::{Cell, RefCell};
use std::path::PathBuf;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib, pango};
use selah_core::books::Section;
use selah_core::import::HeadingKind;
use selah_core::store::{self, BookInfo, Chapter, Translation};

use crate::config;

mod imp {
    use super::*;

    #[derive(Debug, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/cjts15/Selah/ui/window.ui")]
    pub struct SelahWindow {
        #[template_child]
        pub split_view: TemplateChild<adw::NavigationSplitView>,
        #[template_child]
        pub books_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub reader_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub reader_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub scrolled: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub text_view: TemplateChild<gtk::TextView>,
        #[template_child]
        pub error_page: TemplateChild<adw::StatusPage>,
        pub settings: gio::Settings,
        pub books: RefCell<Vec<BookInfo>>,
        /// Book index and chapter most recently requested.
        pub current: Cell<Option<(usize, u32)>>,
    }

    impl Default for SelahWindow {
        fn default() -> Self {
            Self {
                split_view: TemplateChild::default(),
                books_box: TemplateChild::default(),
                reader_page: TemplateChild::default(),
                reader_stack: TemplateChild::default(),
                scrolled: TemplateChild::default(),
                text_view: TemplateChild::default(),
                error_page: TemplateChild::default(),
                settings: gio::Settings::new(config::APP_ID),
                books: RefCell::default(),
                current: Cell::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SelahWindow {
        const NAME: &'static str = "SelahWindow";
        type Type = super::SelahWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();

            klass.install_action("win.previous-chapter", None, |win, _, _| {
                win.step_chapter(false);
            });
            klass.install_action("win.next-chapter", None, |win, _, _| {
                win.step_chapter(true);
            });
            klass.install_action(
                "win.open-chapter",
                Some(&<(String, u32)>::static_variant_type()),
                |win, _, param| {
                    if let Some((book, chapter)) = param.and_then(|p| p.get::<(String, u32)>()) {
                        win.open_chapter_by_id(&book, chapter);
                    }
                },
            );
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for SelahWindow {
        fn constructed(&self) {
            self.parent_constructed();

            let obj = self.obj();
            if config::PROFILE == "development" {
                obj.add_css_class("devel");
            }
            obj.load_window_size();
            obj.setup_text_tags();
            obj.update_navigation();
            obj.load_books();
        }
    }

    impl WidgetImpl for SelahWindow {}

    impl WindowImpl for SelahWindow {
        fn close_request(&self) -> glib::Propagation {
            if let Err(err) = self.obj().save_window_size() {
                glib::g_warning!("selah", "Failed to save window size: {err}");
            }
            self.parent_close_request()
        }
    }

    impl ApplicationWindowImpl for SelahWindow {}
    impl AdwApplicationWindowImpl for SelahWindow {}
}

glib::wrapper! {
    pub struct SelahWindow(ObjectSubclass<imp::SelahWindow>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

fn translation_path() -> PathBuf {
    PathBuf::from(config::PKGDATADIR).join("translations/eng-kjv/translation.sqlite")
}

fn section_title(section: Section) -> String {
    match section {
        Section::OldTestament => gettext("Old Testament"),
        Section::NewTestament => gettext("New Testament"),
        Section::Deuterocanon => gettext("Deuterocanon"),
    }
}

/// Starts a new line unless nothing has been written yet.
fn new_line(buffer: &gtk::TextBuffer, end: &mut gtk::TextIter, first_line: &mut bool) {
    if !*first_line {
        buffer.insert(end, "\n");
    }
    *first_line = false;
}

impl SelahWindow {
    pub fn new(app: &adw::Application) -> Self {
        glib::Object::builder().property("application", app).build()
    }

    fn save_window_size(&self) -> Result<(), glib::BoolError> {
        let settings = &self.imp().settings;
        let (width, height) = self.default_size();
        settings.set_int("window-width", width)?;
        settings.set_int("window-height", height)?;
        settings.set_boolean("is-maximized", self.is_maximized())?;
        Ok(())
    }

    fn load_window_size(&self) {
        let settings = &self.imp().settings;
        self.set_default_size(settings.int("window-width"), settings.int("window-height"));
        if settings.boolean("is-maximized") {
            self.maximize();
        }
    }

    fn setup_text_tags(&self) {
        let buffer = self.imp().text_view.buffer();
        buffer.create_tag(
            Some("verse-number"),
            &[
                ("scale", &0.7_f64),
                ("rise", &(5 * pango::SCALE)),
                ("weight", &700_i32),
            ],
        );
        buffer.create_tag(Some("section"), &[("weight", &700_i32)]);
        buffer.create_tag(Some("title"), &[("style", &pango::Style::Italic)]);
    }

    fn load_books(&self) {
        let path = translation_path();
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = win)]
            self,
            async move {
                let result = gio::spawn_blocking(move || Translation::open(&path)?.books()).await;
                match result {
                    Ok(Ok(books)) => {
                        win.populate_sidebar(&books);
                        *win.imp().books.borrow_mut() = books;
                        win.resume();
                    }
                    Ok(Err(err)) => win.show_error(&err.to_string()),
                    Err(panic) => std::panic::resume_unwind(panic),
                }
            }
        ));
    }

    fn populate_sidebar(&self, books: &[BookInfo]) {
        let mut group: Option<(Section, adw::PreferencesGroup)> = None;
        for book in books {
            if group
                .as_ref()
                .is_none_or(|(section, _)| *section != book.section)
            {
                let new_group = adw::PreferencesGroup::builder()
                    .title(section_title(book.section))
                    .build();
                self.imp().books_box.append(&new_group);
                group = Some((book.section, new_group));
            }

            // Book names come from translation data: never parse them as markup.
            let row = adw::ExpanderRow::builder()
                .title(&book.name)
                .use_markup(false)
                .build();
            let grid = gtk::Grid::builder()
                .column_spacing(6)
                .row_spacing(6)
                .column_homogeneous(true)
                .margin_top(6)
                .margin_bottom(6)
                .margin_start(6)
                .margin_end(6)
                .build();
            row.add_row(&grid);
            if let Some(list_row) = grid.parent().and_downcast::<gtk::ListBoxRow>() {
                list_row.set_activatable(false);
                list_row.set_focusable(false);
            }

            // Chapter buttons are created on first expand; Psalms alone has 150.
            let id = book.id.clone();
            let chapters = book.chapters.clone();
            row.connect_expanded_notify(move |row| {
                if !row.is_expanded() || grid.first_child().is_some() {
                    return;
                }
                for (i, &chapter) in (0_i32..).zip(&chapters) {
                    let button = gtk::Button::builder()
                        .label(chapter.to_string())
                        .action_name("win.open-chapter")
                        .action_target(&(id.as_str(), chapter).to_variant())
                        .build();
                    // Translators: accessible label of a chapter button; {} is the chapter number.
                    let label = gettext("Chapter {}").replace("{}", &chapter.to_string());
                    button.update_property(&[gtk::accessible::Property::Label(&label)]);
                    grid.attach(&button, i % 5, i / 5, 1, 1);
                }
            });

            if let Some((_, group)) = &group {
                group.add(&row);
            }
        }
    }

    fn resume(&self) {
        let imp = self.imp();
        let id = imp.settings.string("last-book");
        let chapter = imp.settings.uint("last-chapter");
        let target = {
            let books = imp.books.borrow();
            books
                .iter()
                .position(|b| b.id == id.as_str() && b.chapters.contains(&chapter))
                .map(|i| (i, chapter))
                .or_else(|| books.first()?.chapters.first().map(|&c| (0, c)))
        };
        if let Some((book, chapter)) = target {
            self.open_chapter(book, chapter);
        }
    }

    fn open_chapter_by_id(&self, id: &str, chapter: u32) {
        let index = self.imp().books.borrow().iter().position(|b| b.id == id);
        if let Some(index) = index {
            self.open_chapter(index, chapter);
        }
    }

    fn step_chapter(&self, forward: bool) {
        let imp = self.imp();
        let target = imp.current.get().and_then(|(book, chapter)| {
            store::step_chapter(&imp.books.borrow(), book, chapter, forward)
        });
        if let Some((book, chapter)) = target {
            self.open_chapter(book, chapter);
        }
    }

    fn open_chapter(&self, index: usize, chapter: u32) {
        let imp = self.imp();
        let Some(book) = imp.books.borrow().get(index).cloned() else {
            return;
        };
        imp.current.set(Some((index, chapter)));
        self.update_navigation();
        imp.split_view.set_show_content(true);

        let saved = imp
            .settings
            .set_string("last-book", &book.id)
            .and_then(|()| imp.settings.set_uint("last-chapter", chapter));
        if let Err(err) = saved {
            glib::g_warning!("selah", "Failed to save reading position: {err}");
        }

        let path = translation_path();
        let id = book.id.clone();
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = win)]
            self,
            async move {
                let result =
                    gio::spawn_blocking(move || Translation::open(&path)?.chapter(&id, chapter))
                        .await;
                // A newer request replaced this one while it was loading.
                if win.imp().current.get() != Some((index, chapter)) {
                    return;
                }
                match result {
                    Ok(Ok(content)) => win.show_chapter(&book, chapter, &content),
                    Ok(Err(err)) => win.show_error(&err.to_string()),
                    Err(panic) => std::panic::resume_unwind(panic),
                }
            }
        ));
    }

    fn update_navigation(&self) {
        let imp = self.imp();
        let (previous, next) = match imp.current.get() {
            Some((book, chapter)) => {
                let books = imp.books.borrow();
                (
                    store::step_chapter(&books, book, chapter, false).is_some(),
                    store::step_chapter(&books, book, chapter, true).is_some(),
                )
            }
            None => (false, false),
        };
        self.action_set_enabled("win.previous-chapter", previous);
        self.action_set_enabled("win.next-chapter", next);
    }

    fn show_chapter(&self, book: &BookInfo, chapter: u32, content: &Chapter) {
        let imp = self.imp();
        let buffer = imp.text_view.buffer();
        buffer.set_text("");
        let mut end = buffer.end_iter();
        let mut first_line = true;
        let heading_tags = |kind: HeadingKind| -> &'static [&'static str] {
            match kind {
                HeadingKind::Title => &["title"],
                HeadingKind::Section => &["section"],
                HeadingKind::Text => &[],
            }
        };

        // All text is inserted as plain text; translation data is never markup.
        let mut headings = content.headings.iter().peekable();
        for verse in &content.verses {
            let mut after_heading = false;
            while let Some(heading) = headings.next_if(|h| h.before_verse <= verse.verse) {
                new_line(&buffer, &mut end, &mut first_line);
                buffer.insert_with_tags_by_name(
                    &mut end,
                    &heading.text,
                    heading_tags(heading.kind),
                );
                after_heading = true;
            }
            if verse.para || after_heading {
                new_line(&buffer, &mut end, &mut first_line);
            } else if first_line {
                first_line = false;
            } else {
                buffer.insert(&mut end, " ");
            }
            buffer.insert_with_tags_by_name(&mut end, &verse.verse.to_string(), &["verse-number"]);
            buffer.insert(&mut end, " ");
            buffer.insert(&mut end, &verse.text);
        }
        for heading in headings {
            new_line(&buffer, &mut end, &mut first_line);
            buffer.insert_with_tags_by_name(&mut end, &heading.text, heading_tags(heading.kind));
        }

        imp.reader_page
            .set_title(&format!("{} {chapter}", book.name));
        imp.reader_stack.set_visible_child_name("reader");
        imp.scrolled.vadjustment().set_value(0.0);
    }

    fn show_error(&self, message: &str) {
        let imp = self.imp();
        // The description is markup; the message may contain paths.
        imp.error_page
            .set_description(Some(&glib::markup_escape_text(message)));
        imp.reader_stack.set_visible_child_name("error");
    }
}
