project(
  'selah',
  version: '0.1.0',
  license: 'GPL-3.0-or-later',
  meson_version: '>= 1.5.0',
)

i18n = import('i18n')
gnome = import('gnome')

base_id = 'io.github.cjts15.Selah'

dependency('gtk4', version: '>= 4.24')
dependency('libadwaita-1', version: '>= 1.10')

cargo = find_program('cargo')
glib_compile_schemas = find_program('glib-compile-schemas')
desktop_file_validate = find_program('desktop-file-validate', required: false)
appstreamcli = find_program('appstreamcli', required: false)

if get_option('profile') == 'development'
  application_id = base_id + '.Devel'
else
  application_id = base_id
endif

gettext_package = meson.project_name()
localedir = get_option('prefix') / get_option('localedir')
pkgdatadir = get_option('prefix') / get_option('datadir') / meson.project_name()

subdir('data')
subdir('src')
subdir('po')

gnome.post_install(
  gtk_update_icon_cache: true,
  glib_compile_schemas: true,
  update_desktop_database: true,
)
