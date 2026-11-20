# ponytail: Meson cannot glob, so edits to the .usfm files alone do not
# trigger a rebuild. In practice the importer is re-copied on every build
# (cargo-build is always stale), which re-runs these targets anyway.
foreach id : ['eng-kjv', 'eng-web']
  custom_target(
    'translation-' + id,
    input: files(id / 'meta.txt'),
    output: id + '.sqlite',
    command: [selah_import, meson.current_source_dir() / id, '@OUTPUT@'],
    build_by_default: true,
    install: true,
    install_dir: pkgdatadir / 'translations',
  )
endforeach
