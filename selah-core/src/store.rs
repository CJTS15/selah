//! Translation databases: one read-only SQLite file per translation.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{Connection, OpenFlags, params};

use crate::Error;
use crate::books::Section;
use crate::import::{Book, Heading, HeadingKind, Meta, Verse};

const SCHEMA_VERSION: i32 = 1;

const SCHEMA: &str = "
CREATE TABLE meta    (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE book    (id TEXT PRIMARY KEY, ord INTEGER NOT NULL, section TEXT NOT NULL,
                      name TEXT NOT NULL, abbrev TEXT NOT NULL);
CREATE TABLE verse   (book TEXT NOT NULL, chapter INTEGER NOT NULL, verse INTEGER NOT NULL,
                      para INTEGER NOT NULL DEFAULT 0, text TEXT NOT NULL,
                      UNIQUE (book, chapter, verse));
CREATE TABLE heading (book TEXT NOT NULL, chapter INTEGER NOT NULL,
                      before_verse INTEGER NOT NULL, kind TEXT NOT NULL, text TEXT NOT NULL);
CREATE INDEX heading_chapter ON heading (book, chapter);
";

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
    pub headings: Vec<Heading>,
}

/// Writes a new translation database at `path`, which must not exist yet.
pub fn create(path: &Path, meta: &Meta, books: &[Book]) -> Result<(), Error> {
    if path.exists() {
        return Err(Error::InvalidData(format!(
            "{} already exists",
            path.display()
        )));
    }
    write(&mut Connection::open(path)?, meta, books)
}

fn write(conn: &mut Connection, meta: &Meta, books: &[Book]) -> Result<(), Error> {
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
            "INSERT INTO verse (book, chapter, verse, para, text) VALUES (?1, ?2, ?3, ?4, ?5)",
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
                insert_verse.execute(params![book.id, v.chapter, v.verse, v.para, v.text])?;
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
    }
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
            "SELECT verse, para, text FROM verse WHERE book = ?1 AND chapter = ?2 ORDER BY verse",
        )?;
        let verses = stmt
            .query_map(params![book, chapter], |row| {
                Ok(Verse {
                    chapter,
                    verse: row.get(0)?,
                    para: row.get(1)?,
                    text: row.get(2)?,
                })
            })?
            .collect::<Result<_, _>>()?;

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
        Ok(Chapter { verses, headings })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::import_dir;

    fn kjv() -> Translation {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/translations/eng-kjv");
        let (meta, books) = import_dir(&dir).unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        write(&mut conn, &meta, &books).unwrap();
        Translation::from_connection(conn).unwrap()
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
                        !v.text.contains(['\\', '|']),
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
            "¶ For God so loved the world, that he gave his only begotten Son, that whosoever \
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
        assert!(kjv.chapter("GEN", 51).unwrap().verses.is_empty());
    }
}
