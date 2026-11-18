mod config;
mod window;

use adw::prelude::*;
use gettextrs::{bind_textdomain_codeset, bindtextdomain, textdomain};
use gtk::{gio, glib};

use crate::window::SelahWindow;

fn main() -> glib::ExitCode {
    bindtextdomain(config::GETTEXT_PACKAGE, config::LOCALEDIR)
        .expect("Unable to bind the text domain");
    bind_textdomain_codeset(config::GETTEXT_PACKAGE, "UTF-8")
        .expect("Unable to set the text domain encoding");
    textdomain(config::GETTEXT_PACKAGE).expect("Unable to switch to the text domain");

    let resources =
        gio::Resource::load(config::RESOURCES_FILE).expect("Could not load the GResource bundle");
    gio::resources_register(&resources);

    let app = adw::Application::builder()
        .application_id(config::APP_ID)
        // Fixed path so the .Devel build finds the same resources.
        .resource_base_path("/io/github/cjts15/Selah")
        .build();

    app.connect_activate(|app| {
        // A second launch activates the running instance; reuse its window.
        let window = app
            .active_window()
            .unwrap_or_else(|| SelahWindow::new(app).upcast());
        window.present();
    });

    // Close windows instead of app.quit() so close-request runs and state is saved.
    let quit = gio::ActionEntry::builder("quit")
        .activate(|app: &adw::Application, _, _| {
            for window in app.windows() {
                window.close();
            }
        })
        .build();
    app.add_action_entries([quit]);
    app.set_accels_for_action("app.quit", &["<Control>q"]);
    app.set_accels_for_action("window.close", &["<Control>w"]);

    app.run()
}
