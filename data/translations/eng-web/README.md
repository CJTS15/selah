# World English Bible (eng-web)

Bundled translation for Selah: the Classic World English Bible with the
full ecumenical book set (Old Testament, Deuterocanon, New Testament). The
text is in the public domain; "World English Bible" is a trademark of
eBible.org, used on condition that the text is not changed. See `copr.htm`.

| | |
| --- | --- |
| Source | https://ebible.org/Scriptures/eng-web_usfm.zip |
| Source files dated | 2026-11-19 |
| Downloaded | 2026-11-20 |
| Zip sha256 | `1d1598aff15b89ee0050f87c48f1be4122bdffb98aa4ae1f9660b471f1815859` |

The `.usfm` files and `copr.htm` are unmodified from the zip. The front
matter (`00-FRTeng-web.usfm`) and glossary (`106-GLOeng-web.usfm`) are left
out because Selah does not display them.

Selah's own files:

- `meta.txt`: metadata for the importer.
- `versification.txt`: where this translation's verse numbers differ from
  the KJV numbering that Selah stores bookmarks and highlights by. Each line
  was checked against the text of both translations.

eBible.org replaces these files in place, so the text is vendored here rather
than downloaded at build time. To update, download the zip, replace the files,
and update the table above.
