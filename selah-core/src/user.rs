//! User data: highlights and bookmarks, keyed by canonical references so they
//! survive removing or switching translations.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};

use crate::Error;

/// Numbered schema migrations; `PRAGMA user_version` counts the applied ones.
const MIGRATIONS: &[&str] = &["
CREATE TABLE annotation (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('bookmark', 'highlight', 'note')),
  ref_start TEXT NOT NULL,
  ref_end TEXT NOT NULL,
  colour TEXT,
  body TEXT,
  created INTEGER NOT NULL,
  modified INTEGER NOT NULL);
CREATE INDEX annotation_ref ON annotation (ref_start);
"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Colour {
    Yellow,
    Green,
    Blue,
    Pink,
    Purple,
}

impl Colour {
    pub const ALL: [Colour; 5] = [
        Colour::Yellow,
        Colour::Green,
        Colour::Blue,
        Colour::Pink,
        Colour::Purple,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Yellow => "yellow",
            Self::Green => "green",
            Self::Blue => "blue",
            Self::Pink => "pink",
            Self::Purple => "purple",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bookmark,
    Highlight,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Bookmark => "bookmark",
            Self::Highlight => "highlight",
        }
    }
}

/// A canonical reference to a whole chapter (`verse` is `None`) or one verse.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Ref {
    pub book: String,
    pub chapter: u32,
    pub verse: Option<u32>,
}

impl Ref {
    /// `JHN.3.16`, or `JHN.3` for a chapter.
    pub fn key(&self) -> String {
        match self.verse {
            Some(verse) => format!("{}.{}.{verse}", self.book, self.chapter),
            None => format!("{}.{}", self.book, self.chapter),
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        let mut parts = key.split('.');
        let book = parts.next().filter(|b| !b.is_empty())?.to_owned();
        let chapter = parts.next()?.parse().ok()?;
        let verse = match parts.next() {
            Some(verse) => Some(verse.parse().ok()?),
            None => None,
        };
        parts.next().is_none().then_some(Self {
            book,
            chapter,
            verse,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    pub id: i64,
    pub kind: Kind,
    pub reference: Ref,
    pub colour: Option<Colour>,
    pub created: i64,
    pub modified: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Bookmarks,
    Colour(Colour),
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

pub struct UserDb {
    conn: Connection,
}

impl UserDb {
    pub fn open(path: &Path) -> Result<Self, Error> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::migrate(conn)
    }

    fn migrate(mut conn: Connection) -> Result<Self, Error> {
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let version = usize::try_from(version).unwrap_or(usize::MAX);
        if version > MIGRATIONS.len() {
            return Err(Error::InvalidData(format!(
                "user data has schema version {version}, newer than this version of Selah"
            )));
        }
        for (applied, sql) in MIGRATIONS.iter().enumerate().skip(version) {
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(
                None,
                "user_version",
                i64::try_from(applied + 1).unwrap_or(i64::MAX),
            )?;
            tx.commit()?;
        }
        Ok(Self { conn })
    }

    /// Highlighted verses of a chapter with their colours.
    pub fn highlights(&self, book: &str, chapter: u32) -> Result<Vec<(u32, Colour)>, Error> {
        let mut stmt = self.conn.prepare(
            "SELECT ref_start, colour FROM annotation WHERE kind = 'highlight' AND ref_start LIKE ?1",
        )?;
        let mut rows = stmt.query(params![format!("{book}.{chapter}.%")])?;
        let mut highlights = Vec::new();
        while let Some(row) = rows.next()? {
            let key: String = row.get(0)?;
            let colour: Option<String> = row.get(1)?;
            if let (
                Some(Ref {
                    verse: Some(verse), ..
                }),
                Some(colour),
            ) = (Ref::parse(&key), colour.as_deref().and_then(Colour::parse))
            {
                highlights.push((verse, colour));
            }
        }
        Ok(highlights)
    }

    pub fn highlight_of(&self, reference: &Ref) -> Result<Option<Colour>, Error> {
        let colour: Option<Option<String>> = self
            .conn
            .query_row(
                "SELECT colour FROM annotation WHERE kind = 'highlight' AND ref_start = ?1",
                params![reference.key()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(colour.flatten().as_deref().and_then(Colour::parse))
    }

    /// Highlights a verse, replacing any colour it had.
    pub fn set_highlight(&mut self, reference: &Ref, colour: Colour) -> Result<(), Error> {
        let key = reference.key();
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM annotation WHERE kind = 'highlight' AND ref_start = ?1",
            params![key],
        )?;
        let time = now();
        tx.execute(
            "INSERT INTO annotation (kind, ref_start, ref_end, colour, created, modified)
             VALUES ('highlight', ?1, ?1, ?2, ?3, ?3)",
            params![key, colour.as_str(), time],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn remove_highlight(&self, reference: &Ref) -> Result<(), Error> {
        self.conn.execute(
            "DELETE FROM annotation WHERE kind = 'highlight' AND ref_start = ?1",
            params![reference.key()],
        )?;
        Ok(())
    }

    pub fn is_bookmarked(&self, reference: &Ref) -> Result<bool, Error> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM annotation WHERE kind = 'bookmark' AND ref_start = ?1",
                params![reference.key()],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn set_bookmark(&self, reference: &Ref, bookmarked: bool) -> Result<(), Error> {
        let key = reference.key();
        self.conn.execute(
            "DELETE FROM annotation WHERE kind = 'bookmark' AND ref_start = ?1",
            params![key],
        )?;
        if bookmarked {
            let time = now();
            self.conn.execute(
                "INSERT INTO annotation (kind, ref_start, ref_end, created, modified)
                 VALUES ('bookmark', ?1, ?1, ?2, ?2)",
                params![key, time],
            )?;
        }
        Ok(())
    }

    /// Annotations matching `filter`, most recently changed first.
    pub fn list(&self, filter: Filter) -> Result<Vec<Annotation>, Error> {
        let (kind, colour) = match filter {
            Filter::All => (None, None),
            Filter::Bookmarks => (Some(Kind::Bookmark.as_str()), None),
            Filter::Colour(colour) => (Some(Kind::Highlight.as_str()), Some(colour.as_str())),
        };
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, ref_start, colour, created, modified FROM annotation
             WHERE kind IN ('bookmark', 'highlight')
               AND (?1 IS NULL OR kind = ?1) AND (?2 IS NULL OR colour = ?2)
             ORDER BY modified DESC, id DESC",
        )?;
        let mut rows = stmt.query(params![kind, colour])?;
        let mut annotations = Vec::new();
        while let Some(row) = rows.next()? {
            let kind: String = row.get(1)?;
            let key: String = row.get(2)?;
            let colour: Option<String> = row.get(3)?;
            let kind = match kind.as_str() {
                "bookmark" => Kind::Bookmark,
                _ => Kind::Highlight,
            };
            // Rows that do not parse are skipped, never deleted.
            let Some(reference) = Ref::parse(&key) else {
                continue;
            };
            annotations.push(Annotation {
                id: row.get(0)?,
                kind,
                reference,
                colour: colour.as_deref().and_then(Colour::parse),
                created: row.get(4)?,
                modified: row.get(5)?,
            });
        }
        Ok(annotations)
    }

    /// Number of bookmarks and of highlights per colour.
    pub fn counts(&self) -> Result<(usize, Vec<(Colour, usize)>), Error> {
        let bookmarks = self.list(Filter::Bookmarks)?.len();
        let mut colours = Vec::new();
        for colour in Colour::ALL {
            let n: i64 = self.conn.query_row(
                "SELECT count(*) FROM annotation WHERE kind = 'highlight' AND colour = ?1",
                params![colour.as_str()],
                |row| row.get(0),
            )?;
            colours.push((colour, usize::try_from(n).unwrap_or(0)));
        }
        Ok((bookmarks, colours))
    }

    pub fn delete(&self, id: i64) -> Result<(), Error> {
        self.conn
            .execute("DELETE FROM annotation WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Puts back a deleted annotation, for undo.
    pub fn restore(&self, annotation: &Annotation) -> Result<(), Error> {
        let key = annotation.reference.key();
        self.conn.execute(
            "INSERT INTO annotation (kind, ref_start, ref_end, colour, created, modified)
             VALUES (?1, ?2, ?2, ?3, ?4, ?5)",
            params![
                annotation.kind.as_str(),
                key,
                annotation.colour.map(Colour::as_str),
                annotation.created,
                annotation.modified
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> UserDb {
        UserDb::migrate(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn verse(book: &str, chapter: u32, verse: u32) -> Ref {
        Ref {
            book: book.into(),
            chapter,
            verse: Some(verse),
        }
    }

    #[test]
    fn ref_keys() {
        assert_eq!(verse("JHN", 3, 16).key(), "JHN.3.16");
        let chapter = Ref {
            book: "JHN".into(),
            chapter: 3,
            verse: None,
        };
        assert_eq!(chapter.key(), "JHN.3");
        assert_eq!(Ref::parse("JHN.3.16"), Some(verse("JHN", 3, 16)));
        assert_eq!(Ref::parse("JHN.3"), Some(chapter));
        for bad in ["", "JHN", "JHN.x", "JHN.3.x", "JHN.3.16.1", ".3"] {
            assert_eq!(Ref::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn migrations_run_once_and_refuse_newer_data() {
        let path = std::env::temp_dir().join(format!("selah-user-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        UserDb::open(&path)
            .unwrap()
            .set_bookmark(&verse("JHN", 3, 16), true)
            .unwrap();
        let reopened = UserDb::open(&path).unwrap();
        assert!(
            reopened.is_bookmarked(&verse("JHN", 3, 16)).unwrap(),
            "data kept on reopen"
        );
        reopened
            .conn
            .pragma_update(
                None,
                "user_version",
                i64::try_from(MIGRATIONS.len() + 1).unwrap(),
            )
            .unwrap();
        drop(reopened);
        assert!(matches!(UserDb::open(&path), Err(Error::InvalidData(_))));
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    #[test]
    fn highlights_replace_and_remove() {
        let mut db = db();
        let jn = verse("JHN", 3, 16);
        db.set_highlight(&jn, Colour::Yellow).unwrap();
        db.set_highlight(&jn, Colour::Green).unwrap();
        db.set_highlight(&verse("JHN", 3, 17), Colour::Blue)
            .unwrap();
        db.set_highlight(&verse("JHN", 30, 1), Colour::Pink)
            .unwrap();
        let mut found = db.highlights("JHN", 3).unwrap();
        found.sort_by_key(|(v, _)| *v);
        assert_eq!(found, vec![(16, Colour::Green), (17, Colour::Blue)]);
        assert_eq!(db.highlight_of(&jn).unwrap(), Some(Colour::Green));
        db.remove_highlight(&jn).unwrap();
        assert_eq!(db.highlight_of(&jn).unwrap(), None);
    }

    #[test]
    fn bookmarks_counts_list_and_undo() {
        let mut db = db();
        let chapter = Ref {
            book: "JHN".into(),
            chapter: 3,
            verse: None,
        };
        db.set_bookmark(&chapter, true).unwrap();
        db.set_bookmark(&verse("JHN", 3, 16), true).unwrap();
        db.set_bookmark(&verse("JHN", 3, 16), true).unwrap();
        assert!(db.is_bookmarked(&chapter).unwrap());
        db.set_highlight(&verse("PSA", 23, 1), Colour::Blue)
            .unwrap();

        let (bookmarks, colours) = db.counts().unwrap();
        assert_eq!(bookmarks, 2, "bookmarking twice keeps one");
        assert!(colours.contains(&(Colour::Blue, 1)));
        assert!(colours.contains(&(Colour::Yellow, 0)));

        let all = db.list(Filter::All).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].reference, verse("PSA", 23, 1), "newest first");
        assert_eq!(db.list(Filter::Colour(Colour::Blue)).unwrap().len(), 1);

        let removed = all[0].clone();
        db.delete(removed.id).unwrap();
        assert_eq!(db.list(Filter::All).unwrap().len(), 2);
        db.restore(&removed).unwrap();
        assert_eq!(
            db.highlight_of(&removed.reference).unwrap(),
            Some(Colour::Blue)
        );

        db.set_bookmark(&chapter, false).unwrap();
        assert!(!db.is_bookmarked(&chapter).unwrap());
    }
}
