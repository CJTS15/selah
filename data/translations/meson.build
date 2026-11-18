# ponytail: Meson cannot glob, so edits to the .usfm files alone do not
# trigger a rebuild. In practice the importer is re-copied on every build
# (cargo-build is always stale), which re-runs this target anyway.
custom_target(
  'translation-eng-kjv',
  input: files('eng-kjv/meta.txt'),
  output: 'translation.sqlite',
  command: [selah_import, meson.current_source_dir() / 'eng-kjv', '@OUTPUT@'],
  build_by_default: true,
  install: true,
  install_dir: pkgdatadir / 'translations' / 'eng-kjv',
)
