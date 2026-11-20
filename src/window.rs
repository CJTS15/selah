use std::cell::{Cell, OnceCell, RefCell};
use std::path::PathBuf;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::{gio, glib, pango};
use selah_core::books::Section;
use selah_core::import::{HeadingKind, Verse};
use selah_core::store::{self, BookInfo, Chapter, Translation};
use selah_core::user::{Annotation, Filter, Ref, UserDb};

use crate::config;

mod catalog;
mod notes;
mod preferences;
mod saved;
mod search;
mod translations;
mod verse;

/// Reader font size at 100% zoom, in points (scaled by system text scaling).
const BASE_POINTS: f64 = 13.0;
const ZOOM_STEP: f64 = 0.1;
// Matches the range of the `text-scale` key.
const ZOOM_MIN: f64 = 0.5;
const ZOOM_MAX: f64 = 3.0;
const GRID_COLUMNS: i32 = 6;

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/cjts15/Selah/ui/window.ui")]
    pub struct SelahWindow {
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub split_view: TemplateChild<adw::NavigationSplitView>,
        #[template_child]
        pub sidebar_mode: TemplateChild<adw::ToggleGroup>,
        #[template_child]
        pub sidebar_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub saved_filters: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub verse_sheet: TemplateChild<adw::BottomSheet>,
        #[template_child]
        pub saved_title: TemplateChild<adw::WindowTitle>,
        #[template_child]
        pub saved_order: TemplateChild<gtk::DropDown>,
        #[template_child]
        pub saved_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub saved_list: TemplateChild<gtk::Box>,
        #[template_child]
        pub sidebar_title: TemplateChild<adw::WindowTitle>,
        #[template_child]
        pub style_follow: TemplateChild<gtk::CheckButton>,
        #[template_child]
        pub style_light: TemplateChild<gtk::CheckButton>,
        #[template_child]
        pub style_dark: TemplateChild<gtk::CheckButton>,
        #[template_child]
        pub zoom_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub book_filter: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        pub book_scroller: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub book_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub reader_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub content_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub search_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        pub scope_bar: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub scope_group: TemplateChild<adw::ToggleGroup>,
        #[template_child]
        pub book_toggle: TemplateChild<adw::Toggle>,
        #[template_child]
        pub search_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub search_results: TemplateChild<gtk::Box>,
        #[template_child]
        pub reader_title: TemplateChild<adw::WindowTitle>,
        #[template_child]
        pub translation_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub sidebar_translation_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub reader_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub scrolled: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub text_view: TemplateChild<gtk::TextView>,
        #[template_child]
        pub error_page: TemplateChild<adw::StatusPage>,
        pub settings: OnceCell<gio::Settings>,
        pub books: RefCell<Vec<BookInfo>>,
        /// Every book of the open translation, including any the preferences hide.
        pub all_books: RefCell<Vec<BookInfo>>,
        /// Bundled translations, by name.
        pub translations: RefCell<Vec<TranslationInfo>>,
        /// The open translation's id; empty until one is open.
        pub translation_id: RefCell<String>,
        pub translation_name: RefCell<String>,
        /// Book index and chapter most recently requested.
        pub current: Cell<Option<(usize, u32)>>,
        /// Book whose chapter grid is open in the sidebar.
        pub expanded: Cell<Option<usize>>,
        /// Chapter button showing the current chapter.
        pub highlighted: RefCell<Option<gtk::Button>>,
        /// Increases with every search so stale results can be dropped.
        pub search_generation: Cell<u64>,
        /// First activatable row in the search results, for the Down key.
        pub first_result: RefCell<Option<gtk::ListBoxRow>>,
        pub translation_abbrev: RefCell<String>,
        /// Highlights and bookmarks; `None` if the user database could not open.
        pub user_db: RefCell<Option<UserDb>>,
        /// The chapter on screen, for copying verse text.
        pub chapter_content: RefCell<Option<Chapter>>,
        /// Verse number with the buffer offsets of its number and text.
        pub verse_ranges: RefCell<Vec<(u32, i32, i32)>>,
        /// Verse chosen by tap or keyboard for the verse actions.
        pub active_verse: Cell<Option<u32>>,
        pub verse_popover: RefCell<Option<gtk::Popover>>,
        pub saved_filter: Cell<Option<Filter>>,
        /// Annotations on the Saved page, for remove and undo.
        pub saved_items: RefCell<Vec<Annotation>>,
        pub saved_generation: Cell<u64>,
        /// The open Translations dialog, refreshed when translations change.
        pub translations_dialog: RefCell<Option<translations::TranslationsDialog>>,
        /// The online catalogue and downloads in progress.
        pub catalog: RefCell<catalog::CatalogState>,
        /// HTTP session, created on first use.
        pub http: OnceCell<soup::Session>,
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
                        win.open_chapter_by_id(&book, chapter, None);
                    }
                },
            );
            klass.install_action(
                "win.open-verse",
                Some(&<(String, u32, u32, u32)>::static_variant_type()),
                |win, _, param| {
                    if let Some((book, chapter, first, last)) =
                        param.and_then(|p| p.get::<(String, u32, u32, u32)>())
                    {
                        win.open_chapter_by_id(&book, chapter, Some((first, last)));
                    }
                },
            );
            klass.install_action("win.search", None, |win, _, _| win.open_search());
            klass.install_action("win.zoom-in", None, |win, _, _| {
                win.set_zoom(win.zoom() + ZOOM_STEP);
            });
            klass.install_action("win.zoom-out", None, |win, _, _| {
                win.set_zoom(win.zoom() - ZOOM_STEP);
            });
            klass.install_action("win.zoom-reset", None, |win, _, _| win.set_zoom(1.0));
            klass.install_action("win.about", None, |win, _, _| win.show_about());
            klass.install_action("win.show-books", None, |win, _, _| {
                win.imp().split_view.set_show_content(false);
            });
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for SelahWindow {
        fn constructed(&self) {
            self.parent_constructed();
            self.settings
                .set(gio::Settings::new(config::APP_ID))
                .expect("settings are set once");

            let obj = self.obj();
            if config::PROFILE == "development" {
                obj.add_css_class("devel");
            }
            obj.load_window_size();
            obj.setup_style();
            obj.setup_text_tags();
            obj.apply_zoom();
            obj.setup_book_list();
            obj.setup_search();
            obj.open_user_db();
            obj.setup_verse_actions();
            obj.setup_saved();
            obj.setup_preferences();
            obj.setup_notes();
            obj.update_navigation();

            self.split_view.connect_collapsed_notify(glib::clone!(
                #[weak]
                obj,
                move |_| obj.update_sidebar_subtitle()
            ));
            // On narrow widths the book list is hidden while reading; bring the
            // current book into view whenever it is shown again.
            self.split_view.connect_show_content_notify(glib::clone!(
                #[weak]
                obj,
                move |split_view| {
                    if !split_view.shows_content()
                        && let Some((book, _)) = obj.imp().current.get()
                    {
                        obj.scroll_to_book(book);
                    }
                }
            ));

            obj.setup_translations();
        }

        fn dispose(&self) {
            // The verse popover is parented to the text view.
            if let Some(popover) = self.verse_popover.take() {
                popover.unparent();
            }
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

const DEFAULT_TRANSLATION: &str = "eng-kjv";

#[derive(Debug, Clone)]
pub struct TranslationInfo {
    pub id: String,
    pub path: PathBuf,
    /// Shipped with the app; cannot be removed.
    pub built_in: bool,
    pub name: String,
    pub abbrev: String,
    /// The language's own name if the metadata has one, else its code.
    pub language: String,
    pub licence: String,
    /// Source date from the catalogue, such as `2026-10-30`; empty if unknown.
    pub version: String,
    pub deuterocanon: bool,
    /// File size in bytes.
    pub size: u64,
    /// Licence, attribution and notice shown in About.
    pub legal: String,
}

fn bundled_dir() -> PathBuf {
    PathBuf::from(config::PKGDATADIR).join("translations")
}

/// Where imported translations are installed.
fn installed_dir() -> PathBuf {
    glib::user_data_dir().join("selah").join("translations")
}

/// Reads the metadata of every bundled and installed translation, sorted by
/// name. A bundled translation wins over an installed file with its id.
fn find_translations() -> Result<Vec<TranslationInfo>, selah_core::Error> {
    let mut found: Vec<TranslationInfo> = Vec::new();
    for (dir, built_in) in [(bundled_dir(), true), (installed_dir(), false)] {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) if !built_in && err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err.into()),
        };
        for entry in entries {
            let path = entry?.path();
            let Some(id) = path
                .extension()
                .filter(|ext| *ext == "sqlite")
                .and(path.file_stem())
                .and_then(|stem| stem.to_str())
                .filter(|id| !found.iter().any(|t| t.id == *id))
            else {
                continue;
            };
            let info = Translation::open(&path).and_then(|translation| {
                let meta = |key: &str| translation.meta(key);
                let name = meta("name")?.unwrap_or_else(|| id.to_owned());
                let mut legal = Vec::new();
                for key in ["licence", "attribution", "notice"] {
                    legal.extend(meta(key)?);
                }
                Ok(TranslationInfo {
                    id: id.to_owned(),
                    built_in,
                    abbrev: meta("abbreviation")?.unwrap_or_else(|| name.clone()),
                    language: match meta("language_name")? {
                        Some(language) => language,
                        None => meta("language")?.unwrap_or_default(),
                    },
                    licence: meta("licence")?.unwrap_or_default(),
                    version: meta("version")?.unwrap_or_default(),
                    deuterocanon: translation
                        .books()?
                        .iter()
                        .any(|b| b.section == Section::Deuterocanon),
                    size: std::fs::metadata(&path).map_or(0, |m| m.len()),
                    legal: legal.join("\n\n"),
                    name,
                    path: path.clone(),
                })
            });
            // One damaged file should not hide the others.
            match info {
                Ok(info) => found.push(info),
                Err(err) => glib::g_warning!("selah", "Skipping {}: {err}", path.display()),
            }
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

/// A toast whose title is plain text: titles carry translation names, which
/// come from imported packages, and toasts parse markup by default.
fn plain_toast(title: &str) -> adw::Toast {
    adw::Toast::builder().title(title).use_markup(false).build()
}

/// The verse number as shown: "16", or "15–16" for a combined verse.
fn verse_label(verse: &Verse) -> String {
    match verse.verse_end {
        Some(last) => format!("{}–{last}", verse.verse),
        None => verse.verse.to_string(),
    }
}

fn section_header(section: Section) -> gtk::Widget {
    let title = match section {
        Section::OldTestament => gettext("Old Testament"),
        Section::NewTestament => gettext("New Testament"),
        Section::Deuterocanon => gettext("Deuterocanon"),
    };
    let header = gtk::Box::builder()
        .spacing(6)
        .margin_top(12)
        .margin_bottom(3)
        .margin_start(12)
        .margin_end(12)
        .build();
    header.append(
        &gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .css_classes(["caption-heading", "dimmed"])
            .build(),
    );
    if section == Section::Deuterocanon {
        header.append(
            &gtk::Label::builder()
                .label(gettext("In this translation"))
                .css_classes(["section-badge", "dimmed"])
                .build(),
        );
    }
    header.upcast()
}

/// "John 3", "John 3:16" or "Psalms 23:1–4".
fn reference_title(book: &str, chapter: u32, verses: Option<(u32, u32)>) -> String {
    match verses {
        None => format!("{book} {chapter}"),
        Some((first, last)) if first == last => format!("{book} {chapter}:{first}"),
        Some((first, last)) => format!("{book} {chapter}:{first}–{last}"),
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

    fn settings(&self) -> &gio::Settings {
        self.imp()
            .settings
            .get()
            .expect("settings are set in constructed")
    }

    fn save_window_size(&self) -> Result<(), glib::BoolError> {
        let settings = self.settings();
        let (width, height) = self.default_size();
        settings.set_int("window-width", width)?;
        settings.set_int("window-height", height)?;
        settings.set_boolean("is-maximized", self.is_maximized())?;
        Ok(())
    }

    fn load_window_size(&self) {
        let settings = self.settings();
        self.set_default_size(settings.int("window-width"), settings.int("window-height"));
        if settings.boolean("is-maximized") {
            self.maximize();
        }
    }

    fn setup_style(&self) {
        let imp = self.imp();
        let apply = |style: &str| {
            adw::StyleManager::default().set_color_scheme(match style {
                "light" => adw::ColorScheme::ForceLight,
                "dark" => adw::ColorScheme::ForceDark,
                _ => adw::ColorScheme::Default,
            });
        };

        let style = self.settings().string("style");
        apply(&style);
        match style.as_str() {
            "light" => imp.style_light.set_active(true),
            "dark" => imp.style_dark.set_active(true),
            _ => imp.style_follow.set_active(true),
        }

        for (button, value) in [
            (&*imp.style_follow, "default"),
            (&*imp.style_light, "light"),
            (&*imp.style_dark, "dark"),
        ] {
            let settings = self.settings().clone();
            button.connect_toggled(move |button| {
                if button.is_active() {
                    apply(value);
                    if let Err(err) = settings.set_string("style", value) {
                        glib::g_warning!("selah", "Failed to save style: {err}");
                    }
                }
            });
        }
    }

    fn setup_text_tags(&self) {
        let buffer = self.imp().text_view.buffer();
        // Later tags take priority, so the base "body" tag comes first. Pango's
        // line-height multiplies the font's own line height (about 1.36 em for Noto Serif).
        buffer.create_tag(Some("body"), &[]);
        buffer.create_tag(Some("verse-number"), &[]);
        buffer.create_tag(Some("section"), &[("weight", &700_i32)]);
        buffer.create_tag(Some("title"), &[("style", &pango::Style::Italic)]);
        self.apply_text_style();
    }

    /// Reading font and line spacing from the preferences.
    pub(super) fn apply_text_style(&self) {
        let settings = self.settings();
        // "Sans" is fontconfig's alias for the system sans-serif font.
        let family = match settings.string("font").as_str() {
            "sans" => "Sans",
            _ => "Noto Serif",
        };
        let line_height: f32 = match settings.string("line-spacing").as_str() {
            "compact" => 1.2,
            "spacious" => 1.6,
            _ => 1.35,
        };
        if let Some(body) = self.imp().text_view.buffer().tag_table().lookup("body") {
            body.set_family(Some(family));
            body.set_line_height(line_height);
        }
    }

    fn zoom(&self) -> f64 {
        self.settings().double("text-scale")
    }

    fn set_zoom(&self, scale: f64) {
        // Round to whole steps so repeated changes do not drift.
        let scale = ((scale / ZOOM_STEP).round() * ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX);
        if let Err(err) = self.settings().set_double("text-scale", scale) {
            glib::g_warning!("selah", "Failed to save text size: {err}");
        }
        self.apply_zoom();
    }

    fn apply_zoom(&self) {
        let imp = self.imp();
        let scale = self.zoom();
        let points = BASE_POINTS * scale;
        let tags = imp.text_view.buffer().tag_table();
        if let Some(body) = tags.lookup("body") {
            body.set_size_points(points);
        }
        if let Some(number) = tags.lookup("verse-number") {
            number.set_size_points(points * 0.6);
            number.set_rise((points * 0.45 * f64::from(pango::SCALE)) as i32);
        }
        imp.zoom_label
            .set_label(&format!("{}%", (scale * 100.0).round()));
        self.action_set_enabled("win.zoom-out", scale > ZOOM_MIN + f64::EPSILON);
        self.action_set_enabled("win.zoom-in", scale < ZOOM_MAX - f64::EPSILON);
        self.action_set_enabled("win.zoom-reset", (scale - 1.0).abs() > f64::EPSILON);
    }

    fn setup_book_list(&self) {
        let imp = self.imp();
        // Row 2n is book n; row 2n + 1 holds its chapter grid.
        imp.book_list.set_filter_func(glib::clone!(
            #[weak(rename_to = win)]
            self,
            #[upgrade_or]
            false,
            move |row| win.row_visible(row)
        ));
        imp.book_list.set_header_func(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |row, before| win.update_header(row, before)
        ));
        imp.book_list.connect_row_activated(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |_, row| win.toggle_book(row.index())
        ));
        imp.book_filter.connect_search_changed(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |_| {
                win.imp().book_list.invalidate_filter();
                win.imp().book_list.invalidate_headers();
            }
        ));
        imp.book_filter
            .connect_stop_search(|entry| entry.set_text(""));
    }

    fn row_visible(&self, row: &gtk::ListBoxRow) -> bool {
        let imp = self.imp();
        let Ok(index) = usize::try_from(row.index()) else {
            return false;
        };
        let book = index / 2;
        let books = imp.books.borrow();
        let Some(info) = books.get(book) else {
            return false;
        };
        let query = imp.book_filter.text().to_lowercase();
        let matches = query.is_empty()
            || info.name.to_lowercase().contains(&query)
            || info.abbrev.to_lowercase().contains(&query);
        if index % 2 == 0 {
            matches
        } else {
            matches && imp.expanded.get() == Some(book)
        }
    }

    fn update_header(&self, row: &gtk::ListBoxRow, before: Option<&gtk::ListBoxRow>) {
        let books = self.imp().books.borrow();
        let section_of = |row: &gtk::ListBoxRow| {
            usize::try_from(row.index())
                .ok()
                .and_then(|i| books.get(i / 2))
                .map(|b| b.section)
        };
        let section = section_of(row);
        let starts_section = section.is_some() && before.and_then(section_of) != section;
        match section {
            Some(section) if starts_section => {
                if row.header().is_none() {
                    row.set_header(Some(&section_header(section)));
                }
            }
            _ => row.set_header(None::<&gtk::Widget>),
        }
    }

    fn populate_sidebar(&self) {
        let imp = self.imp();
        let books = imp.books.borrow();
        for book in books.iter() {
            let n = book.chapters.len();
            let content = gtk::Box::builder().spacing(6).build();
            // Book names come from translation data; Label does not parse markup by default.
            content.append(
                &gtk::Label::builder()
                    .label(&book.name)
                    .xalign(0.0)
                    .hexpand(true)
                    .ellipsize(pango::EllipsizeMode::End)
                    .build(),
            );
            content.append(
                &gtk::Label::builder()
                    .label(n.to_string())
                    .css_classes(["caption", "dimmed", "numeric"])
                    .build(),
            );
            let row = gtk::ListBoxRow::builder().child(&content).build();
            let count = u32::try_from(n).unwrap_or(u32::MAX);
            // Translators: accessible label of a book in the sidebar, e.g. "John, 21 chapters".
            let label = ngettext("{book}, {n} chapter", "{book}, {n} chapters", count)
                .replace("{book}", &book.name)
                .replace("{n}", &n.to_string());
            row.update_property(&[gtk::accessible::Property::Label(&label)]);
            imp.book_list.append(&row);

            let grid = gtk::Grid::builder()
                .css_classes(["chapter-grid"])
                .column_spacing(6)
                .row_spacing(6)
                .column_homogeneous(true)
                .margin_top(6)
                .margin_bottom(6)
                .margin_start(6)
                .margin_end(6)
                .build();
            imp.book_list.append(
                &gtk::ListBoxRow::builder()
                    .child(&grid)
                    .activatable(false)
                    .selectable(false)
                    .focusable(false)
                    .build(),
            );
        }
    }

    /// The chapter grid of `book`, with its buttons created on first use.
    fn chapter_grid(&self, book: usize) -> Option<gtk::Grid> {
        let imp = self.imp();
        let row = imp
            .book_list
            .row_at_index(i32::try_from(book * 2 + 1).ok()?)?;
        let grid = row.child()?.downcast::<gtk::Grid>().ok()?;
        if grid.first_child().is_none() {
            let books = imp.books.borrow();
            let info = books.get(book)?;
            for (i, &chapter) in (0_i32..).zip(&info.chapters) {
                let button = gtk::Button::builder()
                    .label(chapter.to_string())
                    .css_classes(["flat"])
                    .action_name("win.open-chapter")
                    .action_target(&(info.id.as_str(), chapter).to_variant())
                    .build();
                // Translators: accessible label of a chapter button; {} is the chapter number.
                let label = gettext("Chapter {}").replace("{}", &chapter.to_string());
                button.update_property(&[gtk::accessible::Property::Label(&label)]);
                grid.attach(&button, i % GRID_COLUMNS, i / GRID_COLUMNS, 1, 1);
            }
        }
        Some(grid)
    }

    fn toggle_book(&self, index: i32) {
        let imp = self.imp();
        // Odd rows hold chapter grids and are not activatable.
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        if index % 2 == 1 {
            return;
        }
        let book = index / 2;
        let expand = imp.expanded.get() != Some(book);
        imp.expanded.set(expand.then_some(book));
        if expand {
            self.chapter_grid(book);
            self.highlight_current();
        } else {
            imp.book_list.unselect_all();
        }
        imp.book_list.invalidate_filter();
    }

    /// Opens the current book in the sidebar and marks the current chapter.
    fn sync_sidebar(&self) {
        let imp = self.imp();
        let Some((book, _)) = imp.current.get() else {
            return;
        };
        imp.expanded.set(Some(book));
        if let Some(row) = i32::try_from(book * 2)
            .ok()
            .and_then(|i| imp.book_list.row_at_index(i))
        {
            imp.book_list.select_row(Some(&row));
        }
        self.highlight_current();
        imp.book_list.invalidate_filter();
        self.scroll_to_book(book);
    }

    /// Scrolls the sidebar so the book row is in view, once the list has a layout.
    fn scroll_to_book(&self, book: usize) {
        let frames = Cell::new(0_u32);
        // Tick callbacks run once per frame and only while mapped, so this waits
        // for the first layout after startup or after the filter changed rows.
        self.imp().book_list.add_tick_callback(glib::clone!(
            #[weak(rename_to = win)]
            self,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move |list, _| {
                let imp = win.imp();
                let bounds = i32::try_from(book * 2)
                    .ok()
                    .and_then(|i| list.row_at_index(i))
                    .and_then(|row| row.compute_bounds(list));
                let Some(bounds) = bounds.filter(|b| b.height() > 0.0) else {
                    frames.set(frames.get() + 1);
                    return if frames.get() < 60 {
                        glib::ControlFlow::Continue
                    } else {
                        glib::ControlFlow::Break
                    };
                };
                let adjustment = imp.book_scroller.vadjustment();
                let top = f64::from(bounds.y());
                let bottom = top + f64::from(bounds.height());
                if top < adjustment.value() || bottom > adjustment.value() + adjustment.page_size()
                {
                    adjustment.set_value(top - 48.0);
                }
                glib::ControlFlow::Break
            }
        ));
    }

    fn highlight_current(&self) {
        let imp = self.imp();
        if let Some(old) = imp.highlighted.take() {
            old.remove_css_class("suggested-action");
            old.add_css_class("flat");
        }
        let Some((book, chapter)) = imp.current.get() else {
            return;
        };
        let Some(position) = imp
            .books
            .borrow()
            .get(book)
            .and_then(|b| b.chapters.iter().position(|&c| c == chapter))
        else {
            return;
        };
        let Some(grid) = self.chapter_grid(book) else {
            return;
        };
        let mut child = grid.first_child();
        for _ in 0..position {
            child = child.and_then(|c| c.next_sibling());
        }
        if let Some(button) = child.and_downcast::<gtk::Button>() {
            button.remove_css_class("flat");
            button.add_css_class("suggested-action");
            imp.highlighted.replace(Some(button));
        }
    }

    fn update_sidebar_subtitle(&self) {
        let imp = self.imp();
        let subtitle = if imp.split_view.is_collapsed() {
            imp.translation_name.borrow().clone()
        } else {
            String::new()
        };
        imp.sidebar_title.set_subtitle(&subtitle);
    }

    /// The open translation's database file.
    pub(super) fn translation_path(&self) -> PathBuf {
        let imp = self.imp();
        let id = imp.translation_id.borrow();
        imp.translations
            .borrow()
            .iter()
            .find(|t| t.id == *id)
            .map(|t| t.path.clone())
            .unwrap_or_default()
    }

    fn setup_translations(&self) {
        // The radio items of the picker menu follow this action's state.
        let action = gio::SimpleAction::new_stateful(
            "translation",
            Some(glib::VariantTy::STRING),
            &"".to_variant(),
        );
        action.connect_activate(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |_, param| {
                if let Some(id) = param.and_then(|p| p.str()) {
                    win.switch_translation(id);
                }
            }
        ));
        self.add_action(&action);
        self.setup_translation_actions();
        self.reload_translations();
    }

    /// Finds the translations again, after start-up, an import or a removal.
    pub(super) fn reload_translations(&self) {
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = win)]
            self,
            async move {
                match gio::spawn_blocking(find_translations).await {
                    Ok(Ok(found)) if !found.is_empty() => win.show_translations(found),
                    Ok(Ok(_)) => win.show_error(&gettext("No translations are installed")),
                    Ok(Err(err)) => win.show_error(&err.to_string()),
                    Err(panic) => std::panic::resume_unwind(panic),
                }
            }
        ));
    }

    fn show_translations(&self, found: Vec<TranslationInfo>) {
        let imp = self.imp();
        let menu = gio::Menu::new();
        for info in &found {
            // Menu labels treat `_` as a mnemonic marker; keep names literal.
            let item = gio::MenuItem::new(Some(&info.name.replace('_', "__")), None);
            item.set_action_and_target_value(Some("win.translation"), Some(&info.id.to_variant()));
            menu.append_item(&item);
        }
        for button in [&*imp.translation_button, &*imp.sidebar_translation_button] {
            button.set_menu_model(Some(&menu));
        }

        let current = imp.translation_id.borrow().clone();
        let still_open = found.iter().any(|t| t.id == current);
        // Only a found id is used, never the stored text as a path.
        let wanted = self.settings().string("translation");
        let id = [wanted.as_str(), DEFAULT_TRANSLATION]
            .into_iter()
            .find(|id| found.iter().any(|t| t.id == *id))
            .unwrap_or(&found[0].id)
            .to_owned();
        *imp.translations.borrow_mut() = found;
        self.refresh_translations_dialog();
        if !still_open {
            self.open_translation(&id, None);
        }
    }

    /// Opens another translation at the same place, as far as it has it.
    pub(super) fn switch_translation(&self, id: &str) {
        if *self.imp().translation_id.borrow() == id {
            return;
        }
        // The open chapter's canonical reference, so the old file is not needed.
        let place = self.canonical_ref(None);
        self.open_translation(id, Some(place));
    }

    /// Loads translation `id`. `place` is a canonical chapter or verse to open
    /// in it (`Some(None)` when switching without one); `None` resumes the
    /// last position.
    pub(super) fn open_translation(&self, id: &str, place: Option<Option<Ref>>) {
        let imp = self.imp();
        let Some(info) = imp
            .translations
            .borrow()
            .iter()
            .find(|t| t.id == id)
            .cloned()
        else {
            return;
        };
        *imp.translation_id.borrow_mut() = info.id.clone();
        let path = self.translation_path();
        let switched = place.is_some();
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = win)]
            self,
            async move {
                let result = gio::spawn_blocking(move || {
                    let translation = Translation::open(&path)?;
                    let target = match place.flatten() {
                        Some(canonical) => translation.from_canonical(&canonical)?,
                        None => None,
                    };
                    Ok::<_, selah_core::Error>((translation.books()?, target))
                })
                .await;
                // Another translation was chosen while this one loaded.
                if *win.imp().translation_id.borrow() != info.id {
                    return;
                }
                match result {
                    Ok(Ok((books, target))) => win.show_translation(&info, books, target, switched),
                    Ok(Err(err)) => win.show_error(&err.to_string()),
                    Err(panic) => std::panic::resume_unwind(panic),
                }
            }
        ));
    }

    fn show_translation(
        &self,
        info: &TranslationInfo,
        books: Vec<BookInfo>,
        target: Option<Ref>,
        switched: bool,
    ) {
        let imp = self.imp();
        if let Err(err) = self.settings().set_string("translation", &info.id) {
            glib::g_warning!("selah", "Failed to save the translation: {err}");
        }
        if let Some(action) = self.lookup_action("translation") {
            action.change_state(&info.id.to_variant());
        }
        // Translators: accessible label of the translation picker; {name} is a translation.
        let label = gettext("Translation: {name}").replace("{name}", &info.name);
        for button in [&*imp.translation_button, &*imp.sidebar_translation_button] {
            button.set_label(&info.abbrev);
            button.update_property(&[gtk::accessible::Property::Label(&label)]);
        }
        imp.reader_title.set_subtitle(&info.name);
        *imp.translation_name.borrow_mut() = info.name.clone();
        *imp.translation_abbrev.borrow_mut() = info.abbrev.clone();
        self.update_sidebar_subtitle();

        imp.book_list.remove_all();
        imp.expanded.set(None);
        imp.highlighted.replace(None);
        imp.current.set(None);
        *imp.all_books.borrow_mut() = books.clone();
        let mut books = books;
        if !self.settings().boolean("show-deuterocanon") {
            books.retain(|b| b.section != Section::Deuterocanon);
        }
        *imp.books.borrow_mut() = books;
        self.populate_sidebar();

        // A place in a book that is hidden or missing falls back to the first book.
        let target = target.filter(|t| imp.books.borrow().iter().any(|b| b.id == t.book));
        match target {
            Some(target) => {
                self.open_chapter_by_id(&target.book, target.chapter, target.verse.map(|v| (v, v)));
            }
            // The place is missing here: start at the first book.
            None if switched => {
                let first = imp
                    .books
                    .borrow()
                    .first()
                    .and_then(|b| b.chapters.first().copied());
                if let Some(chapter) = first {
                    self.open_chapter(0, chapter, None);
                }
            }
            None => self.resume(),
        }
        self.refresh_saved();
        if imp.content_stack.visible_child_name().as_deref() == Some("search") {
            self.run_search();
        }
    }

    fn resume(&self) {
        let imp = self.imp();
        let id = self.settings().string("last-book");
        let chapter = self.settings().uint("last-chapter");
        let target = {
            let books = imp.books.borrow();
            books
                .iter()
                .position(|b| b.id == id.as_str() && b.chapters.contains(&chapter))
                .map(|i| (i, chapter))
                .or_else(|| books.first()?.chapters.first().map(|&c| (0, c)))
        };
        if let Some((book, chapter)) = target {
            self.open_chapter(book, chapter, None);
        }
    }

    /// Opens a chapter by book id; false if the book is not in the list.
    fn open_chapter_by_id(&self, id: &str, chapter: u32, verses: Option<(u32, u32)>) -> bool {
        let index = self.imp().books.borrow().iter().position(|b| b.id == id);
        if let Some(index) = index {
            self.open_chapter(index, chapter, verses);
        }
        index.is_some()
    }

    fn step_chapter(&self, forward: bool) {
        let imp = self.imp();
        let target = imp.current.get().and_then(|(book, chapter)| {
            store::step_chapter(&imp.books.borrow(), book, chapter, forward)
        });
        if let Some((book, chapter)) = target {
            self.open_chapter(book, chapter, None);
        }
    }

    /// Opens a chapter, optionally scrolled to and selecting a range of verses.
    fn open_chapter(&self, index: usize, chapter: u32, verses: Option<(u32, u32)>) {
        let imp = self.imp();
        let Some(book) = imp.books.borrow().get(index).cloned() else {
            return;
        };
        imp.current.set(Some((index, chapter)));
        // Verse actions wait for the new chapter's canonical references.
        *imp.chapter_content.borrow_mut() = None;
        // A jump from the search page remembers what was searched.
        if imp.content_stack.visible_child_name().as_deref() == Some("search") {
            self.remember_search();
        }
        imp.content_stack.set_visible_child_name("reading");
        self.update_navigation();
        self.update_chapter_bookmark();
        self.sync_sidebar();
        imp.split_view.set_show_content(true);

        let settings = self.settings();
        let saved = settings
            .set_string("last-book", &book.id)
            .and_then(|()| settings.set_uint("last-chapter", chapter));
        if let Err(err) = saved {
            glib::g_warning!("selah", "Failed to save reading position: {err}");
        }

        let path = self.translation_path();
        let id = book.id.clone();
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = win)]
            self,
            async move {
                let loaded_path = path.clone();
                let result =
                    gio::spawn_blocking(move || Translation::open(&path)?.chapter(&id, chapter))
                        .await;
                // A newer request or another translation replaced this one.
                if win.imp().current.get() != Some((index, chapter))
                    || win.translation_path() != loaded_path
                {
                    return;
                }
                match result {
                    Ok(Ok(content)) => win.show_chapter(&book, chapter, &content, verses),
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

    fn show_chapter(
        &self,
        book: &BookInfo,
        chapter: u32,
        content: &Chapter,
        verses: Option<(u32, u32)>,
    ) {
        let imp = self.imp();
        let buffer = imp.text_view.buffer();
        buffer.set_text("");
        let mut end = buffer.end_iter();
        let mut first_line = true;
        let numbers = self.settings().boolean("verse-numbers");
        let one_per_line = self.settings().string("verse-layout") == "lines";
        let notes = self.note_verses(content);
        // Character offsets of the requested verses, for selection.
        let mut selection: Option<(i32, i32)> = None;
        let mut ranges = Vec::with_capacity(content.verses.len());
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
            if verse.para || after_heading || one_per_line {
                new_line(&buffer, &mut end, &mut first_line);
            } else if first_line {
                first_line = false;
            } else {
                buffer.insert(&mut end, " ");
            }
            let verse_start = end.offset();
            if numbers {
                buffer.insert_with_tags_by_name(&mut end, &verse_label(verse), &["verse-number"]);
                // Thin space between the number and the verse.
                buffer.insert(&mut end, "\u{2009}");
            }
            buffer.insert(&mut end, &verse.text);
            if notes.contains(&verse.verse) {
                buffer.insert(&mut end, "\u{2009}");
                let anchor = buffer.create_child_anchor(&mut end);
                imp.text_view
                    .add_child_at_anchor(&self.note_mark(verse.verse), &anchor);
            }
            ranges.push((verse.verse, verse_start, end.offset()));
            if let Some((first, last)) = verses
                && (first..=last).contains(&verse.verse)
            {
                let start = selection.map_or(verse_start, |(start, _)| start);
                selection = Some((start, end.offset()));
            }
        }
        for heading in headings {
            new_line(&buffer, &mut end, &mut first_line);
            buffer.insert_with_tags_by_name(&mut end, &heading.text, heading_tags(heading.kind));
        }
        let (start, end) = buffer.bounds();
        buffer.apply_tag_by_name("body", &start, &end);
        *imp.verse_ranges.borrow_mut() = ranges;
        *imp.chapter_content.borrow_mut() = Some(content.clone());
        imp.active_verse.set(None);
        self.apply_highlights();
        self.update_chapter_bookmark();

        let title = format!("{} {chapter}", book.name);
        imp.reader_page.set_title(&title);
        imp.reader_title.set_title(&title);
        imp.reader_stack.set_visible_child_name("reader");
        match selection {
            Some((start, end)) => {
                let start = buffer.iter_at_offset(start);
                buffer.select_range(&start, &buffer.iter_at_offset(end));
                // select_range puts the insert mark at the verse start; GTK may
                // defer this scroll until the new text has a layout.
                imp.text_view
                    .scroll_to_mark(&buffer.get_insert(), 0.0, true, 0.0, 0.2);
                imp.text_view.grab_focus();
            }
            None => imp.scrolled.vadjustment().set_value(0.0),
        }
    }

    /// The small button after a verse with a note, which opens the note.
    fn note_mark(&self, verse: u32) -> gtk::Button {
        let button = gtk::Button::builder()
            .icon_name("document-edit-symbolic")
            .tooltip_text(gettext("Note"))
            .valign(gtk::Align::Center)
            .css_classes(["flat", "circular", "note-mark"])
            .build();
        if let Some(title) = self.verse_title(verse) {
            // Translators: accessible label of the note mark; {reference} is a verse.
            let label = gettext("Note on {reference}").replace("{reference}", &title);
            button.update_property(&[gtk::accessible::Property::Label(&label)]);
        }
        button.connect_clicked(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |_| win.open_note_dialog(verse)
        ));
        button
    }

    fn show_error(&self, message: &str) {
        glib::g_warning!("selah", "{message}");
        let imp = self.imp();
        // The description is markup; the message may contain paths.
        imp.error_page
            .set_description(Some(&glib::markup_escape_text(message)));
        imp.reader_stack.set_visible_child_name("error");
    }

    fn show_about(&self) {
        let imp = self.imp();
        let about = adw::AboutDialog::builder()
            .application_name("Selah")
            .application_icon(config::APP_ID)
            .developer_name("CJTS15")
            .version(config::VERSION)
            .website("https://github.com/CJTS15/selah")
            .issue_url("https://github.com/CJTS15/selah/issues")
            .copyright("© 2026 CJTS15")
            .license_type(gtk::License::Gpl30)
            .build();
        for info in imp.translations.borrow().iter() {
            // Legal text is markup; translation metadata is plain text.
            let legal = glib::markup_escape_text(&info.legal);
            about.add_legal_section(&info.name, None, gtk::License::Custom, Some(&legal));
        }
        about.present(Some(self));
    }
}
