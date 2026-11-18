use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};

use crate::config;

mod imp {
    use super::*;

    #[derive(Debug, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/cjts15/Selah/ui/window.ui")]
    pub struct SelahWindow {
        pub settings: gio::Settings,
    }

    impl Default for SelahWindow {
        fn default() -> Self {
            Self {
                settings: gio::Settings::new(config::APP_ID),
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
}
