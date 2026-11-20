blueprints = custom_target(
  'blueprints',
  input: files(
    'preferences-dialog.blp',
    'note-dialog.blp',
    'shortcuts-dialog.blp',
    'translations-dialog.blp',
    'window.blp',
  ),
  output: '.',
  command: [
    find_program('blueprint-compiler'),
    'batch-compile',
    '@OUTPUT@',
    '@CURRENT_SOURCE_DIR@',
    '@INPUT@',
  ],
)
