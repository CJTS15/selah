//! Translation databases: one read-only SQLite file per translation.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::config::DbConfig;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::Error;
use crate::books::{self, Section};
use crate::import::{self, Book, Heading, HeadingKind, Meta, Verse};
use crate::user::Ref;
use crate::versification::{self, Mapping};

const SCHEMA_VERSION: i32 = 3;

const SCHEMA: &str = "
CREATE TABLE meta    (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE book    (id TEXT PRIMARY KEY, ord INTEGER NOT NULL, section TEXT NOT NULL,
                      name TEXT NOT NULL, abbrev TEXT NOT NULL);
CREATE TABLE verse   (book TEXT NOT NULL, chapter INTEGER NOT NULL, verse INTEGER NOT NULL,
                      verse_end INTEGER, para INTEGER NOT NULL DEFAULT 0, text TEXT NOT NULL,
                      UNIQUE (book, chapter, verse));
CREATE TABLE verse_map (book TEXT NOT NULL, chapter INTEGER NOT NULL, verse INTEGER NOT NULL,
                      canon_book TEXT, canon_chapter INTEGER, canon_verse INTEGER,
                      UNIQUE (book, chapter, verse));
CREATE INDEX verse_map_canon ON verse_map (canon_book, canon_chapter, canon_verse);
CREATE TABLE heading (book TEXT NOT NULL, chapter INTEGER NOT NULL,
                      before_verse INTEGER NOT NULL, kind TEXT NOT NULL, text TEXT NOT NULL);
CREATE INDEX heading_chapter ON heading (book, chapter);
CREATE VIRTUAL TABLE verse_fts USING fts5(text, content='verse', content_rowid='rowid',
                      tokenize='unicode61 remove_diacritics 2');
";

/// Marks the start of a match in [`SearchHit::snippet`].
pub const MATCH_START: char = '\u{1}';
/// Marks the end of a match in [`SearchHit::snippet`].
pub const MATCH_END: char = '\u{2}';

/// Which verses a search covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    All,
    Section(Section),
    Book(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub book: String,
    pub chapter: u32,
    pub verse: u32,
    /// Verse text, shortened around the match, with matches between
    /// [`MATCH_START`] and [`MATCH_END`].
    pub snippet: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchResults {
    /// The first hits in Bible order, up to the requested limit.
    pub hits: Vec<SearchHit>,
    /// Number of matching verses per section, in Bible order.
    pub counts: Vec<(Section, usize)>,
}

impl SearchResults {
    pub fn total(&self) -> usize {
        self.counts.iter().map(|(_, n)| n).sum()
    }
}

/// How [`Translation::search`] matches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchOptions {
    /// The last word must match whole instead of as a prefix.
    pub whole_words: bool,
    /// Leave out the Deuterocanon (the "Show Deuterocanon" preference).
    pub hide_deuterocanon: bool,
}

/// Turns typed text into an FTS5 phrase query, with the last word as a prefix
/// unless `whole_words`: `everlasting li` -> `"everlasting li"*`. Only letters
/// and digits survive, so user text can never act as FTS5 syntax. `None` if no
/// words remain.
pub fn fts_query(text: &str, whole_words: bool) -> Option<String> {
    let words: Vec<&str> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let prefix = if whole_words { "" } else { "*" };
    (!words.is_empty()).then(|| format!("\"{}\"{prefix}", words.join(" ")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookInfo {
    pub id: String,
    pub section: Section,
    pub name: String,
    pub abbrev: String,
    pub chapters: Vec<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chapter {
    pub verses: Vec<Verse>,
    /// The canonical reference of each verse in `verses`, if it has one.
    pub canonical: Vec<Option<Ref>>,
    pub headings: Vec<Heading>,
}

/// Writes a new translation database at `path`, which must not exist yet.
pub fn create(path: &Path, meta: &Meta, books: &[Book], mapping: &Mapping) -> Result<(), Error> {
    if path.exists() {
        return Err(Error::InvalidData(format!(
            "{} already exists",
            path.display()
        )));
    }
    let result = Connection::open(path)
        .map_err(Error::from)
        .and_then(|mut conn| write(&mut conn, meta, books, mapping));
    if result.is_err() {
        // A failed write leaves no partial file behind.
        let _ = std::fs::remove_file(path);
    }
    result
}

/// `canon_book`, `canon_chapter`, `canon_verse` of a `verse_map` row.
type CanonicalColumns = (Option<String>, Option<u32>, Option<u32>);

/// Checks an untrusted translation database, such as one unpacked from a
/// package, and writes a clean copy to `out`, which must not exist. Only the
/// rows Selah reads are copied, so triggers, views or extra tables in the
/// input never reach the copy. Returns the copy's metadata.
pub fn rebuild(untrusted: &Path, out: &Path) -> Result<Meta, Error> {
    let invalid = |message: String| Error::InvalidData(message);
    let conn = Connection::open_with_flags(
        untrusted,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    // Hardening for databases from elsewhere (sqlite.org/security.html).
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false)?;

    let check: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if check != "ok" {
        return Err(invalid(format!("damaged database: {check}")));
    }
    let version: i32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != SCHEMA_VERSION {
        return Err(invalid(format!("unsupported schema version {version}")));
    }
    for table in ["meta", "book", "verse", "heading", "verse_map"] {
        let kind: Option<String> = conn
            .query_row(
                "SELECT type FROM sqlite_schema WHERE name = ?1",
                [table],
                |row| row.get(0),
            )
            .optional()?;
        if kind.as_deref() != Some("table") {
            return Err(invalid(format!("missing table {table}")));
        }
    }

    let mut stmt = conn.prepare("SELECT key, value FROM meta")?;
    let meta: Meta = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    import::check_meta(&meta)?;

    let mut stmt = conn.prepare("SELECT id, name, abbrev FROM book ORDER BY ord")?;
    let rows: Vec<(String, String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<_, _>>()?;
    let mut verse_stmt = conn.prepare(
        "SELECT chapter, verse, verse_end, para, text FROM verse
         WHERE book = ?1 ORDER BY chapter, verse",
    )?;
    let mut heading_stmt = conn.prepare(
        "SELECT chapter, before_verse, kind, text FROM heading
         WHERE book = ?1 ORDER BY chapter, before_verse, rowid",
    )?;
    let mut books = Vec::with_capacity(rows.len());
    for (id, name, abbrev) in rows {
        // The section comes from the id, never from the input.
        let section =
            books::section(&id).ok_or_else(|| invalid(format!("unknown book id {id:?}")))?;
        let verses: Vec<Verse> = verse_stmt
            .query_map([&id], |row| {
                Ok(Verse {
                    chapter: row.get(0)?,
                    verse: row.get(1)?,
                    verse_end: row.get(2)?,
                    para: row.get(3)?,
                    text: row.get(4)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        if let Some(bad) = verses.iter().find(|v| {
            v.chapter == 0
                || v.verse == 0
                || v.verse_end.is_some_and(|end| end <= v.verse)
                || v.text.is_empty()
        }) {
            return Err(invalid(format!(
                "bad verse {id} {}:{}",
                bad.chapter, bad.verse
            )));
        }
        let mut headings = Vec::new();
        let mut rows = heading_stmt.query([&id])?;
        while let Some(row) = rows.next()? {
            let kind: String = row.get(2)?;
            headings.push(Heading {
                chapter: row.get(0)?,
                before_verse: row.get(1)?,
                kind: HeadingKind::parse(&kind)
                    .ok_or_else(|| invalid(format!("heading kind {kind:?}")))?,
                text: row.get(3)?,
            });
        }
        books.push(Book {
            id,
            section,
            name,
            abbrev,
            verses,
            headings,
        });
    }

    let mut stmt = conn.prepare(
        "SELECT book, chapter, verse, canon_book, canon_chapter, canon_verse FROM verse_map",
    )?;
    let rows: Vec<(Ref, CanonicalColumns)> = stmt
        .query_map([], |row| {
            let from = Ref {
                book: row.get(0)?,
                chapter: row.get(1)?,
                verse: Some(row.get(2)?),
            };
            Ok((from, (row.get(3)?, row.get(4)?, row.get(5)?)))
        })?
        .collect::<Result<_, _>>()?;
    let mut mapping = Mapping::with_capacity(rows.len());
    for (from, (book, chapter, verse)) in rows {
        let to = match (book, chapter, verse) {
            (None, None, None) => None,
            (Some(book), Some(chapter), Some(verse))
                if books::section(&book).is_some() && chapter > 0 && verse > 0 =>
            {
                Some(Ref {
                    book,
                    chapter,
                    verse: Some(verse),
                })
            }
            _ => return Err(invalid(format!("bad versification entry {}", from.key()))),
        };
        mapping.push((from, to));
    }
    versification::check_mapping(&books, &mapping)?;

    create(out, &meta, &books, &mapping)?;
    Ok(meta)
}

fn write(
    conn: &mut Connection,
    meta: &Meta,
    books: &[Book],
    mapping: &Mapping,
) -> Result<(), Error> {
    let tx = conn.transaction()?;
    tx.execute_batch(SCHEMA)?;
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    {
        let mut insert_meta = tx.prepare("INSERT INTO meta (key, value) VALUES (?1, ?2)")?;
        for (key, value) in meta {
            insert_meta.execute(params![key, value])?;
        }

        let mut insert_book = tx.prepare(
            "INSERT INTO book (id, ord, section, name, abbrev) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        let mut insert_verse = tx.prepare(
            "INSERT INTO verse (book, chapter, verse, verse_end, para, text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        let mut insert_heading = tx.prepare(
            "INSERT INTO heading (book, chapter, before_verse, kind, text)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for (ord, book) in (0_i64..).zip(books) {
            insert_book.execute(params![
                book.id,
                ord,
                book.section.as_str(),
                book.name,
                book.abbrev
            ])?;
            for v in &book.verses {
                insert_verse.execute(params![
                    book.id,
                    v.chapter,
                    v.verse,
                    v.verse_end,
                    v.para,
                    v.text
                ])?;
            }
            for h in &book.headings {
                insert_heading.execute(params![
                    book.id,
                    h.chapter,
                    h.before_verse,
                    h.kind.as_str(),
                    h.text
                ])?;
            }
        }

        let mut insert_map = tx.prepare(
            "INSERT INTO verse_map (book, chapter, verse, canon_book, canon_chapter, canon_verse)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for (from, to) in mapping {
            insert_map.execute(params![
                from.book,
                from.chapter,
                from.verse,
                to.as_ref().map(|r| &r.book),
                to.as_ref().map(|r| r.chapter),
                to.as_ref().and_then(|r| r.verse)
            ])?;
        }
    }
    tx.execute("INSERT INTO verse_fts (verse_fts) VALUES ('rebuild')", [])?;
    tx.commit()?;
    Ok(())
}

/// The chapter before or after `chapter` in `books[book]`, crossing into the
/// neighbouring book at either end. `None` at the first or last chapter.
pub fn step_chapter(
    books: &[BookInfo],
    book: usize,
    chapter: u32,
    forward: bool,
) -> Option<(usize, u32)> {
    let chapters = &books.get(book)?.chapters;
    let pos = chapters.iter().position(|&c| c == chapter)?;
    if forward {
        if let Some(&next) = chapters.get(pos + 1) {
            return Some((book, next));
        }
        books
            .iter()
            .enumerate()
            .skip(book + 1)
            .find_map(|(i, b)| b.chapters.first().map(|&c| (i, c)))
    } else {
        if pos > 0 {
            return Some((book, chapters[pos - 1]));
        }
        books[..book]
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, b)| b.chapters.last().map(|&c| (i, c)))
    }
}

pub struct Translation {
    conn: Connection,
}

impl Translation {
    pub fn open(path: &Path) -> Result<Self, Error> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        Self::from_connection(conn)
    }

    fn from_connection(conn: Connection) -> Result<Self, Error> {
        let version: i32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version != SCHEMA_VERSION {
            return Err(Error::InvalidData(format!(
                "unsupported schema version {version}"
            )));
        }
        Ok(Self { conn })
    }

    /// The text of one verse, or `None` if this translation does not have it.
    pub fn verse_text(
        &self,
        book: &str,
        chapter: u32,
        verse: u32,
    ) -> Result<Option<String>, Error> {
        Ok(self
            .conn
            .query_row(
                "SELECT text FROM verse WHERE book = ?1 AND chapter = ?2 AND verse = ?3",
                params![book, chapter, verse],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// A value from the translation's metadata, such as `name` or `licence`.
    pub fn meta(&self, key: &str) -> Result<Option<String>, Error> {
        let mut stmt = self.conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        Ok(match rows.next()? {
            Some(row) => Some(row.get(0)?),
            None => None,
        })
    }

    /// Verses matching `text` as a phrase (see [`fts_query`]) within `scope`.
    pub fn search(
        &self,
        text: &str,
        scope: &Scope,
        limit: usize,
        options: SearchOptions,
    ) -> Result<SearchResults, Error> {
        let Some(query) = fts_query(text, options.whole_words) else {
            return Ok(SearchResults::default());
        };
        let (section, book) = match scope {
            Scope::All => (None, None),
            Scope::Section(section) => (Some(section.as_str()), None),
            Scope::Book(book) => (None, Some(book.as_str())),
        };
        const FROM: &str = "FROM verse_fts JOIN verse v ON v.rowid = verse_fts.rowid
             JOIN book b ON b.id = v.book
             WHERE verse_fts MATCH ?1 AND (?2 IS NULL OR b.section = ?2)
               AND (?3 IS NULL OR v.book = ?3) AND (?5 = 0 OR b.section <> 'DC')";

        let mut stmt = self.conn.prepare(&format!(
            "SELECT b.section, count(*) {FROM} GROUP BY b.section ORDER BY min(b.ord)"
        ))?;
        // ?4 is the limit, used only by the second query.
        let hide = options.hide_deuterocanon;
        let mut rows = stmt.query(params![query, section, book, 0, hide])?;
        let mut counts = Vec::new();
        while let Some(row) = rows.next()? {
            let section: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            counts.push((
                Section::parse(&section)
                    .ok_or_else(|| Error::InvalidData(format!("section {section:?}")))?,
                usize::try_from(count).unwrap_or(0),
            ));
        }

        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut stmt = self.conn.prepare(&format!(
            "SELECT v.book, v.chapter, v.verse,
                    snippet(verse_fts, 0, char(1), char(2), '…', 48)
             {FROM} ORDER BY b.ord, v.chapter, v.verse LIMIT ?4"
        ))?;
        let hits = stmt
            .query_map(params![query, section, book, limit, hide], |row| {
                let snippet: String = row.get(3)?;
                Ok(SearchHit {
                    book: row.get(0)?,
                    chapter: row.get(1)?,
                    verse: row.get(2)?,
                    // Words of a phrase are marked one by one; join them into one match.
                    snippet: snippet.replace("\u{2} \u{1}", " "),
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(SearchResults { hits, counts })
    }

    /// Books in the translation's order, each with its chapter numbers.
    pub fn books(&self) -> Result<Vec<BookInfo>, Error> {
        let mut chapters: HashMap<String, Vec<u32>> = HashMap::new();
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT book, chapter FROM verse ORDER BY book, chapter")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            chapters.entry(row.get(0)?).or_default().push(row.get(1)?);
        }

        let mut stmt = self
            .conn
            .prepare("SELECT id, section, name, abbrev FROM book ORDER BY ord")?;
        let mut rows = stmt.query([])?;
        let mut books = Vec::new();
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let section: String = row.get(1)?;
            books.push(BookInfo {
                section: Section::parse(&section)
                    .ok_or_else(|| Error::InvalidData(format!("section {section:?}")))?,
                name: row.get(2)?,
                abbrev: row.get(3)?,
                chapters: chapters.remove(&id).unwrap_or_default(),
                id,
            });
        }
        Ok(books)
    }

    pub fn chapter(&self, book: &str, chapter: u32) -> Result<Chapter, Error> {
        let mut stmt = self.conn.prepare(
            "SELECT v.verse, v.verse_end, v.para, v.text,
                    m.rowid IS NOT NULL, m.canon_book, m.canon_chapter, m.canon_verse
             FROM verse v LEFT JOIN verse_map m
               ON m.book = v.book AND m.chapter = v.chapter AND m.verse = v.verse
             WHERE v.book = ?1 AND v.chapter = ?2 ORDER BY v.verse",
        )?;
        let rows: Vec<(Verse, Option<Ref>)> = stmt
            .query_map(params![book, chapter], |row| {
                let verse = Verse {
                    chapter,
                    verse: row.get(0)?,
                    verse_end: row.get(1)?,
                    para: row.get(2)?,
                    text: row.get(3)?,
                };
                let mapped: bool = row.get(4)?;
                let canonical = if mapped {
                    canonical_ref(row.get(5)?, row.get(6)?, row.get(7)?)
                } else {
                    Some(Ref {
                        book: book.to_owned(),
                        chapter,
                        verse: Some(verse.verse),
                    })
                };
                Ok((verse, canonical))
            })?
            .collect::<Result<_, _>>()?;
        let (verses, canonical) = rows.into_iter().unzip();

        let mut stmt = self.conn.prepare(
            "SELECT before_verse, kind, text FROM heading
             WHERE book = ?1 AND chapter = ?2 ORDER BY before_verse, rowid",
        )?;
        let mut rows = stmt.query(params![book, chapter])?;
        let mut headings = Vec::new();
        while let Some(row) = rows.next()? {
            let kind: String = row.get(1)?;
            headings.push(Heading {
                chapter,
                before_verse: row.get(0)?,
                kind: HeadingKind::parse(&kind)
                    .ok_or_else(|| Error::InvalidData(format!("heading kind {kind:?}")))?,
                text: row.get(2)?,
            });
        }
        Ok(Chapter {
            verses,
            canonical,
            headings,
        })
    }

    /// The canonical reference of a verse or chapter in this translation, or
    /// `None` if it has none. A chapter maps through its first verse.
    pub fn to_canonical(&self, r: &Ref) -> Result<Option<Ref>, Error> {
        let Some(verse) = r.verse else {
            let first: Option<u32> = self.conn.query_row(
                "SELECT min(verse) FROM verse WHERE book = ?1 AND chapter = ?2",
                params![r.book, r.chapter],
                |row| row.get(0),
            )?;
            let Some(first) = first else { return Ok(None) };
            let canonical = self.to_canonical(&Ref {
                verse: Some(first),
                ..r.clone()
            })?;
            return Ok(canonical.map(|c| Ref { verse: None, ..c }));
        };
        let mapped = self
            .conn
            .query_row(
                "SELECT canon_book, canon_chapter, canon_verse FROM verse_map
                 WHERE book = ?1 AND chapter = ?2 AND verse = ?3",
                params![r.book, r.chapter, verse],
                |row| Ok(canonical_ref(row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        Ok(mapped.unwrap_or_else(|| Some(r.clone())))
    }

    /// Where a canonical verse or chapter is in this translation, or `None` if
    /// it is not. A verse inside a combined verse (`15-16`) finds that verse.
    /// A chapter maps through its verse 1, else to the same chapter.
    pub fn from_canonical(&self, r: &Ref) -> Result<Option<Ref>, Error> {
        let Some(verse) = r.verse else {
            if let Some(found) = self.from_canonical(&Ref {
                verse: Some(1),
                ..r.clone()
            })? {
                return Ok(Some(Ref {
                    verse: None,
                    ..found
                }));
            }
            let exists = self
                .conn
                .query_row(
                    "SELECT 1 FROM verse WHERE book = ?1 AND chapter = ?2 LIMIT 1",
                    params![r.book, r.chapter],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            return Ok(exists.then(|| r.clone()));
        };
        let mapped = self
            .conn
            .query_row(
                "SELECT book, chapter, verse FROM verse_map
                 WHERE canon_book = ?1 AND canon_chapter = ?2 AND canon_verse = ?3",
                params![r.book, r.chapter, verse],
                |row| {
                    Ok(Ref {
                        book: row.get(0)?,
                        chapter: row.get(1)?,
                        verse: Some(row.get(2)?),
                    })
                },
            )
            .optional()?;
        if mapped.is_some() {
            return Ok(mapped);
        }
        // The same number maps to itself unless the mapping moved it away.
        let found = self
            .conn
            .query_row(
                "SELECT verse FROM verse v
                 WHERE book = ?1 AND chapter = ?2 AND verse <= ?3
                   AND coalesce(verse_end, verse) >= ?3
                   AND NOT EXISTS (SELECT 1 FROM verse_map m
                       WHERE m.book = v.book AND m.chapter = v.chapter AND m.verse = ?3)",
                params![r.book, r.chapter, verse],
                |row| row.get(0),
            )
            .optional()?;
        Ok(found.map(|verse| Ref {
            verse: Some(verse),
            ..r.clone()
        }))
    }
}

fn canonical_ref(book: Option<String>, chapter: Option<u32>, verse: Option<u32>) -> Option<Ref> {
    Some(Ref {
        book: book?,
        chapter: chapter?,
        verse: Some(verse?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::import_dir;

    fn kjv() -> Translation {
        bundled("eng-kjv")
    }

    fn bundled(id: &str) -> Translation {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../data/translations")
            .join(id);
        let (meta, books, mapping) = import_dir(&dir).unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        write(&mut conn, &meta, &books, &mapping).unwrap();
        Translation::from_connection(conn).unwrap()
    }

    #[test]
    fn fts_query_is_a_quoted_phrase() {
        assert_eq!(
            fts_query("everlasting li", false).as_deref(),
            Some("\"everlasting li\"*")
        );
        assert_eq!(
            fts_query("everlasting li", true).as_deref(),
            Some("\"everlasting li\"")
        );
        assert_eq!(
            fts_query("  \"NEAR(a b)\" OR c* ", false).as_deref(),
            Some("\"NEAR a b OR c\"*")
        );
        assert_eq!(fts_query("¿?!", false).as_deref(), None);
    }

    #[test]
    fn kjv_search() {
        let kjv = kjv();
        let found = kjv
            .search(
                "everlasting life",
                &Scope::All,
                200,
                SearchOptions::default(),
            )
            .unwrap();
        assert_eq!(found.counts[0], (Section::OldTestament, 1), "only Dan 12:2");
        assert_eq!(
            (
                found.hits[0].book.as_str(),
                found.hits[0].chapter,
                found.hits[0].verse
            ),
            ("DAN", 12, 2)
        );
        assert!(
            found.hits[0].snippet.contains("\u{1}everlasting life\u{2}"),
            "{}",
            found.hits[0].snippet
        );
        assert_eq!(found.total(), found.hits.len());

        let prefix = kjv
            .search("Everlasting LI", &Scope::All, 200, SearchOptions::default())
            .unwrap();
        assert!(prefix.total() >= found.total());

        let john = kjv
            .search(
                "begotten son",
                &Scope::Book("JHN".into()),
                200,
                SearchOptions::default(),
            )
            .unwrap();
        assert!(john.hits.iter().all(|h| h.book == "JHN"));
        assert!(john.hits.iter().any(|h| (h.chapter, h.verse) == (3, 16)));

        let nt = kjv
            .search(
                "everlasting life",
                &Scope::Section(Section::NewTestament),
                3,
                SearchOptions::default(),
            )
            .unwrap();
        assert_eq!(nt.hits.len(), 3, "limited");
        assert_eq!(nt.counts.len(), 1);
        assert!(nt.total() > 3);

        // Typed FTS5 syntax is treated as words, never as an expression.
        assert!(
            kjv.search("\"NEAR( OR *", &Scope::All, 10, SearchOptions::default())
                .is_ok()
        );

        // Whole words: "love" no longer finds "loved"; hiding the Deuterocanon
        // drops its section from counts and hits.
        let prefix = kjv
            .search("love", &Scope::All, 10_000, SearchOptions::default())
            .unwrap();
        let whole = SearchOptions {
            whole_words: true,
            ..SearchOptions::default()
        };
        let exact = kjv.search("love", &Scope::All, 10_000, whole).unwrap();
        assert!(exact.total() < prefix.total());
        assert!(exact.hits.iter().all(|h| !h.snippet.contains("\u{1}loved")));
        let hidden = SearchOptions {
            hide_deuterocanon: true,
            ..SearchOptions::default()
        };
        let no_dc = kjv.search("love", &Scope::All, 10_000, hidden).unwrap();
        assert!(
            prefix
                .counts
                .iter()
                .any(|(s, _)| *s == Section::Deuterocanon)
        );
        assert!(
            no_dc
                .counts
                .iter()
                .all(|(s, _)| *s != Section::Deuterocanon)
        );
        assert!(
            no_dc
                .hits
                .iter()
                .all(|h| crate::books::section(&h.book) != Some(Section::Deuterocanon))
        );
        assert_eq!(
            kjv.search("…", &Scope::All, 10, SearchOptions::default())
                .unwrap(),
            SearchResults::default()
        );
    }

    #[test]
    fn step_chapter_crosses_books() {
        let book = |id: &str, chapters: Vec<u32>| BookInfo {
            id: id.into(),
            section: Section::OldTestament,
            name: id.into(),
            abbrev: id.into(),
            chapters,
        };
        let books = [
            book("MAL", vec![1, 2, 3, 4]),
            book("ESG", vec![10, 11]),
            book("MAT", vec![1]),
        ];
        assert_eq!(step_chapter(&books, 0, 4, true), Some((1, 10)));
        assert_eq!(step_chapter(&books, 1, 10, true), Some((1, 11)));
        assert_eq!(step_chapter(&books, 1, 11, true), Some((2, 1)));
        assert_eq!(step_chapter(&books, 2, 1, true), None);
        assert_eq!(step_chapter(&books, 2, 1, false), Some((1, 11)));
        assert_eq!(step_chapter(&books, 1, 10, false), Some((0, 4)));
        assert_eq!(step_chapter(&books, 0, 1, false), None);
        assert_eq!(step_chapter(&books, 0, 9, true), None, "unknown chapter");
        assert_eq!(step_chapter(&books, 7, 1, true), None, "unknown book");
    }

    #[test]
    fn kjv_counts_and_text() {
        let kjv = kjv();
        let books = kjv.books().unwrap();
        assert_eq!(books.len(), 80);
        assert_eq!(books[0].id, "GEN");
        assert_eq!(books[39].id, "TOB", "Apocrypha sits between the testaments");
        assert_eq!(books[79].id, "REV");

        let mut verses = HashMap::new();
        let mut chapters = HashMap::new();
        for book in &books {
            for &c in &book.chapters {
                let chapter = kjv.chapter(&book.id, c).unwrap();
                *verses.entry(book.section).or_insert(0) += chapter.verses.len();
                *chapters.entry(book.section).or_insert(0) += 1;
                for v in &chapter.verses {
                    assert!(!v.text.is_empty(), "{} {c}:{} is empty", book.id, v.verse);
                    assert!(
                        !v.text.contains(['\\', '|', '¶']),
                        "{} {c}:{} has markup: {}",
                        book.id,
                        v.verse,
                        v.text
                    );
                }
            }
        }
        let ot_nt =
            |m: &HashMap<Section, usize>| m[&Section::OldTestament] + m[&Section::NewTestament];
        assert_eq!(ot_nt(&verses), 31_102);
        assert_eq!(ot_nt(&chapters), 1_189);
        assert_eq!(verses[&Section::Deuterocanon], 5_720);
        assert_eq!(chapters[&Section::Deuterocanon], 173);

        let john3 = kjv.chapter("JHN", 3).unwrap();
        assert_eq!(
            john3.verses[15].text,
            "For God so loved the world, that he gave his only begotten Son, that whosoever \
             believeth in him should not perish, but have everlasting life."
        );

        let psalm3 = kjv.chapter("PSA", 3).unwrap();
        assert_eq!(psalm3.headings[0].kind, HeadingKind::Title);
        assert_eq!(psalm3.headings[0].before_verse, 1);
        assert_eq!(
            psalm3.headings[0].text,
            "A Psalm of David, when he fled from Absalom his son."
        );

        let romans16 = kjv.chapter("ROM", 16).unwrap();
        let subscription = romans16.headings.last().unwrap();
        assert_eq!(subscription.before_verse, 28, "after the last verse");

        assert_eq!(kjv.chapter("ESG", 10).unwrap().verses[0].verse, 4);
        assert_eq!(
            kjv.meta("name").unwrap().as_deref(),
            Some("King James Version")
        );
        assert!(
            kjv.meta("notice")
                .unwrap()
                .unwrap()
                .contains("Letters Patent")
        );
        assert_eq!(kjv.meta("missing").unwrap(), None);
        assert!(
            kjv.verse_text("JHN", 3, 16)
                .unwrap()
                .unwrap()
                .starts_with("For God so loved")
        );
        assert_eq!(kjv.verse_text("JHN", 3, 99).unwrap(), None);
        assert!(kjv.chapter("GEN", 51).unwrap().verses.is_empty());
    }

    #[test]
    fn canonical_mapping() {
        use crate::import::parse_usfm;
        use crate::versification::parse_mapping;

        let books = vec![
            parse_usfm("\\id ROM\n\\c 14\n\\v 23 Sin.\n\\v 24 Now to him.\n\\c 16\n\\v 24 Grace.\n\\v 25 \\f + \\ft TR.\\f*\n")
                .unwrap(),
            parse_usfm("\\id SIR\n\\c 11\n\\v 14 Good.\n\\v 15-16 Combined.\n").unwrap(),
        ];
        let mapping = parse_mapping("ROM 14:24 = ROM 16:25\nROM 14:23 = none").unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        let meta = vec![("id".to_owned(), "test".to_owned())];
        write(&mut conn, &meta, &books, &mapping).unwrap();
        let t = Translation::from_connection(conn).unwrap();

        let r = |key: &str| Ref::parse(key).unwrap();
        let to = |key: &str| t.to_canonical(&r(key)).unwrap().map(|r| r.key());
        let from = |key: &str| t.from_canonical(&r(key)).unwrap().map(|r| r.key());

        assert_eq!(to("ROM.14.24").as_deref(), Some("ROM.16.25"));
        assert_eq!(to("ROM.14.23"), None);
        assert_eq!(to("ROM.16.24").as_deref(), Some("ROM.16.24"));
        assert_eq!(to("ROM.16").as_deref(), Some("ROM.16"));
        assert_eq!(to("ROM.15"), None);

        assert_eq!(from("ROM.16.25").as_deref(), Some("ROM.14.24"));
        assert_eq!(from("ROM.14.24"), None, "moved away");
        assert_eq!(from("ROM.14.23"), None, "no canonical ref");
        assert_eq!(
            from("SIR.11.16").as_deref(),
            Some("SIR.11.15"),
            "inside a range"
        );
        assert_eq!(from("SIR.11").as_deref(), Some("SIR.11"));
        assert_eq!(from("ROM.15"), None);

        let chapter = t.chapter("ROM", 14).unwrap();
        assert_eq!(chapter.canonical, vec![None, Some(r("ROM.16.25"))]);
        assert_eq!(t.chapter("SIR", 11).unwrap().verses[1].verse_end, Some(16));
    }

    #[test]
    fn web_versification() {
        let web = bundled("eng-web");
        let r = |key: &str| Ref::parse(key).unwrap();
        let from = |key: &str| web.from_canonical(&r(key)).unwrap().map(|r| r.key());
        let to = |key: &str| web.to_canonical(&r(key)).unwrap().map(|r| r.key());

        assert_eq!(from("JHN.3.16").as_deref(), Some("JHN.3.16"));
        assert_eq!(from("ROM.16.25").as_deref(), Some("ROM.14.24"));
        assert_eq!(from("MAT.23.13").as_deref(), Some("MAT.23.14"));
        assert_eq!(from("ACT.8.37"), None, "omitted by WEB");
        assert_eq!(from("2ES.7.36").as_deref(), Some("2ES.7.106"));
        assert_eq!(to("2ES.7.50"), None, "fragment the KJV lacks");
        assert_eq!(from("S3Y.1.45"), None);
        assert_eq!(from("S3Y.1.56").as_deref(), Some("DAG.3.77"));
        assert_eq!(from("SUS.1").as_deref(), Some("DAG.13"));
        assert_eq!(to("DAG.14").as_deref(), Some("BEL.1"));
        assert_eq!(from("ESG.11.1").as_deref(), Some("ESG.10.14"));
        assert_eq!(from("SIR.33.31").as_deref(), Some("SIR.33.32"));
        // The only combined verse with text.
        assert_eq!(from("4MA.8.29").as_deref(), Some("4MA.8.28"));

        // Every mapping target is a real KJV verse, so no key is a typo.
        let kjv = kjv();
        let mut stmt = web
            .conn
            .prepare("SELECT canon_book, canon_chapter, canon_verse FROM verse_map WHERE canon_book IS NOT NULL")
            .unwrap();
        let targets: Vec<(String, u32, u32)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(targets.len() > 200);
        for (book, chapter, verse) in targets {
            assert!(
                kjv.verse_text(&book, chapter, verse).unwrap().is_some(),
                "{book} {chapter}:{verse}"
            );
        }
    }

    /// A fresh path in the temp directory, removed first.
    fn temp_path(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("selah-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    /// A small translation file with a combined verse, a heading and a mapping.
    fn small_translation(name: &str) -> std::path::PathBuf {
        use crate::import::{parse_meta, parse_usfm};
        use crate::versification::parse_mapping;

        let path = temp_path(name);
        let meta =
            parse_meta("id=test\nname=Test\nlanguage=en\ndirection=ltr\nlicence=PD\n").unwrap();
        let books = vec![
            parse_usfm("\\id ROM\n\\c 14\n\\s1 Judging\n\\p\n\\v 23 Sin.\n\\v 24 Now to him.\n\\c 16\n\\v 24 Grace.\n")
                .unwrap(),
            parse_usfm("\\id SIR\n\\c 11\n\\v 14 Good.\n\\v 15-16 Combined.\n").unwrap(),
        ];
        let mapping = parse_mapping("ROM 14:24 = ROM 16:25\nROM 14:23 = none").unwrap();
        create(&path, &meta, &books, &mapping).unwrap();
        path
    }

    fn tamper(path: &Path, sql: &str) {
        Connection::open(path).unwrap().execute_batch(sql).unwrap();
    }

    #[test]
    fn rebuild_round_trip() {
        let source = small_translation("round-src");
        let out = temp_path("round-out");
        let meta = rebuild(&source, &out).unwrap();
        assert!(meta.contains(&("id".into(), "test".into())));

        let (a, b) = (
            Translation::open(&source).unwrap(),
            Translation::open(&out).unwrap(),
        );
        assert_eq!(a.books().unwrap(), b.books().unwrap());
        for (book, chapter) in [("ROM", 14), ("ROM", 16), ("SIR", 11)] {
            assert_eq!(
                a.chapter(book, chapter).unwrap(),
                b.chapter(book, chapter).unwrap()
            );
        }
        assert!(
            b.search("combined", &Scope::All, 10, SearchOptions::default())
                .unwrap()
                .total()
                == 1,
            "index rebuilt"
        );
        assert!(rebuild(&source, &out).is_err(), "never overwrites");
        let _ = std::fs::remove_file(source);
        let _ = std::fs::remove_file(out);
    }

    #[test]
    fn rebuild_drops_extra_schema() {
        let source = small_translation("extra-src");
        tamper(
            &source,
            "CREATE TABLE evil (x); CREATE VIEW v AS SELECT 1;
             CREATE TRIGGER t AFTER INSERT ON verse BEGIN DELETE FROM verse; END;",
        );
        let out = temp_path("extra-out");
        rebuild(&source, &out).unwrap();
        let names: Vec<String> = Connection::open(&out)
            .unwrap()
            .prepare("SELECT name FROM sqlite_schema WHERE name IN ('evil', 'v', 't')")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(names.is_empty(), "{names:?}");
        let _ = std::fs::remove_file(source);
        let _ = std::fs::remove_file(out);
    }

    #[test]
    fn rebuild_refuses_bad_input() {
        for (name, sql) in [
            ("version", "PRAGMA user_version = 2;"),
            ("id", "UPDATE meta SET value = '../x' WHERE key = 'id';"),
            ("licence", "DELETE FROM meta WHERE key = 'licence';"),
            (
                "book",
                "UPDATE book SET id = 'XYZ' WHERE id = 'SIR'; UPDATE verse SET book = 'XYZ' WHERE book = 'SIR';",
            ),
            ("verse", "UPDATE verse SET text = '' WHERE verse = 14;"),
            ("type", "UPDATE verse SET chapter = 'one' WHERE verse = 14;"),
            (
                "view",
                "DROP TABLE verse_map; CREATE VIEW verse_map AS SELECT * FROM verse;",
            ),
            (
                "mapping",
                "INSERT INTO verse_map VALUES ('ROM', 16, 24, 'ROM', 16, 25);",
            ),
        ] {
            let source = small_translation(&format!("bad-{name}"));
            tamper(&source, sql);
            let out = temp_path(&format!("bad-{name}-out"));
            assert!(rebuild(&source, &out).is_err(), "{name} accepted");
            assert!(!out.exists(), "{name} left a file");
            let _ = std::fs::remove_file(source);
        }

        let garbage = temp_path("garbage");
        std::fs::write(&garbage, b"not a database at all").unwrap();
        let out = temp_path("garbage-out");
        assert!(rebuild(&garbage, &out).is_err());
        assert!(!out.exists());
        let _ = std::fs::remove_file(garbage);
    }
}
