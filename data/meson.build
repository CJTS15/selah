conf = configuration_data()
conf.set('APP_ID', application_id)
conf.set('APP_PATH', '/' + application_id.replace('.', '/') + '/')
conf.set('GETTEXT_PACKAGE', gettext_package)

desktop_file = i18n.merge_file(
  input: configure_file(
    input: 'selah.desktop.in.in',
    output: 'selah.desktop.in',
    configuration: conf,
  ),
  output: application_id + '.desktop',
  type: 'desktop',
  po_dir: '../po',
  install: true,
  install_dir: get_option('datadir') / 'applications',
)

if desktop_file_validate.found()
  test('validate-desktop', desktop_file_validate, args: [desktop_file])
endif

metainfo_file = i18n.merge_file(
  input: configure_file(
    input: 'selah.metainfo.xml.in.in',
    output: 'selah.metainfo.xml.in',
    configuration: conf,
  ),
  output: application_id + '.metainfo.xml',
  po_dir: '../po',
  install: true,
  install_dir: get_option('datadir') / 'metainfo',
)

if appstreamcli.found()
  test(
    'validate-metainfo',
    appstreamcli,
    args: ['validate', '--no-net', '--explain', metainfo_file],
  )
endif

configure_file(
  input: 'selah.gschema.xml.in',
  output: application_id + '.gschema.xml',
  configuration: conf,
  install: true,
  install_dir: get_option('datadir') / 'glib-2.0' / 'schemas',
)

test(
  'validate-schema',
  glib_compile_schemas,
  args: ['--strict', '--dry-run', meson.current_build_dir()],
)

subdir('icons')
subdir('translations')
subdir('ui')

gnome.compile_resources(
  'resources',
  'resources.gresource.xml',
  gresource_bundle: true,
  source_dir: [meson.current_build_dir(), meson.current_source_dir()],
  install: true,
  install_dir: pkgdatadir,
  dependencies: blueprints,
)
