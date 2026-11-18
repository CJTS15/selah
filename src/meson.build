cargo_options = [
  '--manifest-path', meson.project_source_root() / 'Cargo.toml',
  '--target-dir', meson.project_build_root() / 'src',
]

if get_option('profile') == 'default'
  cargo_options += ['--release']
  rust_target = 'release'
else
  rust_target = 'debug'
endif

# Read by src/config.rs through env!().
cargo_env = [
  'APP_ID=' + application_id,
  'GETTEXT_PACKAGE=' + gettext_package,
  'LOCALEDIR=' + localedir,
  'PKGDATADIR=' + pkgdatadir,
  'PROFILE=' + get_option('profile'),
  'VERSION=' + meson.project_version(),
]

# Builds the app and selah-import; only the app is installed. data/ runs
# selah-import to create the bundled translation database.
cargo_build = custom_target(
  'cargo-build',
  build_by_default: true,
  build_always_stale: true,
  output: [meson.project_name(), 'selah-import'],
  console: true,
  install: true,
  install_dir: [get_option('bindir'), false],
  command: [
    'env', cargo_env,
    cargo, 'build', '--workspace', cargo_options,
    '&&',
    'cp', 'src' / rust_target / meson.project_name(), '@OUTPUT0@',
    '&&',
    'cp', 'src' / rust_target / 'selah-import', '@OUTPUT1@',
  ],
)
selah_import = cargo_build[1]
