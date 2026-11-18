// Values come from Meson (src/meson.build) through the environment of `cargo build`.
pub const APP_ID: &str = env!("APP_ID");
pub const GETTEXT_PACKAGE: &str = env!("GETTEXT_PACKAGE");
pub const LOCALEDIR: &str = env!("LOCALEDIR");
pub const PROFILE: &str = env!("PROFILE");
pub const RESOURCES_FILE: &str = concat!(env!("PKGDATADIR"), "/resources.gresource");
