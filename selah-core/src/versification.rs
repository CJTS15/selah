//! Versification: how a translation's verse numbers map to the canonical
//! scheme (KJV versification) that user data is keyed by.
//!
//! Each translation lists only its exceptions in `versification.txt`, in the
//! mapping syntax of Paratext `.vrs` files, translation side first:
//!
//! ```text
//! # comment
//! ROM 14:24-26 = ROM 16:25-27
//! 2ES 7:36-105 = none
//! ```
//!
//! `none` means the verses have no canonical reference. Every verse not
//! listed maps to itself. The lookups live in [`crate::store::Translation`].

use std::collections::{HashMap, HashSet};

use crate::Error;
use crate::books;
use crate::import::Book;
use crate::user::Ref;

/// One entry per translation verse: its canonical verse, or `None`.
pub type Mapping = Vec<(Ref, Option<Ref>)>;

pub fn parse_mapping(src: &str) -> Result<Mapping, Error> {
    let mut mapping = Mapping::new();
    for (index, line) in src.lines().enumerate() {
        let line = line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        let error = |message: &str| {
            Error::InvalidData(format!("versification line {}: {message}", index + 1))
        };
        let (from, to) = line.split_once('=').ok_or_else(|| error("expected `=`"))?;
        let from = parse_span(from).ok_or_else(|| error("bad reference"))?;
        if to.trim() == "none" {
            mapping.extend(from.verses().map(|r| (r, None)));
            continue;
        }
        let to = parse_span(to).ok_or_else(|| error("bad reference"))?;
        if from.last - from.first != to.last - to.first {
            return Err(error("ranges differ in length"));
        }
        mapping.extend(from.verses().zip(to.verses().map(Some)));
    }
    Ok(mapping)
}

/// Rejects a mapping that names a verse the translation lacks, or that gives
/// two verses the same canonical reference (counting unlisted verses, which
/// map to themselves).
pub fn check_mapping(books: &[Book], mapping: &Mapping) -> Result<(), Error> {
    let listed: HashMap<&Ref, &Option<Ref>> = mapping.iter().map(|(from, to)| (from, to)).collect();
    if listed.len() != mapping.len() {
        return Err(Error::InvalidData(
            "versification lists a verse twice".into(),
        ));
    }

    let mut present = HashSet::new();
    let mut seen: HashMap<Ref, Ref> = HashMap::new();
    for book in books {
        for verse in &book.verses {
            // A combined verse (`\v 15-16`) holds every number in its range.
            for number in verse.verse..=verse.verse_end.unwrap_or(verse.verse) {
                let from = Ref {
                    book: book.id.clone(),
                    chapter: verse.chapter,
                    verse: Some(number),
                };
                let canonical = match listed.get(&from) {
                    Some(to) => (*to).clone(),
                    None => Some(from.clone()),
                };
                if let Some(canonical) = canonical
                    && let Some(other) = seen.insert(canonical.clone(), from.clone())
                {
                    return Err(Error::InvalidData(format!(
                        "versification maps {} and {} to {}",
                        other.key(),
                        from.key(),
                        canonical.key()
                    )));
                }
                present.insert(from);
            }
        }
    }
    if let Some((missing, _)) = mapping.iter().find(|(from, _)| !present.contains(from)) {
        return Err(Error::InvalidData(format!(
            "versification names {}, which the translation lacks",
            missing.key()
        )));
    }
    Ok(())
}

struct Span {
    book: String,
    chapter: u32,
    first: u32,
    last: u32,
}

impl Span {
    fn verses(&self) -> impl Iterator<Item = Ref> + '_ {
        (self.first..=self.last).map(|verse| Ref {
            book: self.book.clone(),
            chapter: self.chapter,
            verse: Some(verse),
        })
    }
}

/// `ROM 14:24-26` or `ROM 14:24`.
fn parse_span(s: &str) -> Option<Span> {
    let (book, numbers) = s.trim().split_once(' ')?;
    books::section(book)?;
    let (chapter, verses) = numbers.trim().split_once(':')?;
    let (first, last) = verses.split_once('-').unwrap_or((verses, verses));
    let (first, last): (u32, u32) = (first.parse().ok()?, last.parse().ok()?);
    (first > 0 && last >= first).then_some(Span {
        book: book.to_owned(),
        chapter: chapter.parse().ok().filter(|&c| c > 0)?,
        first,
        last,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::parse_usfm;

    fn r(book: &str, chapter: u32, verse: u32) -> Ref {
        Ref {
            book: book.into(),
            chapter,
            verse: Some(verse),
        }
    }

    #[test]
    fn parses_ranges_and_none() {
        let mapping = parse_mapping(
            "# Romans doxology\nROM 14:24-26 = ROM 16:25-27\n2ES 7:36 = none # fragment\n",
        )
        .unwrap();
        assert_eq!(
            mapping,
            vec![
                (r("ROM", 14, 24), Some(r("ROM", 16, 25))),
                (r("ROM", 14, 25), Some(r("ROM", 16, 26))),
                (r("ROM", 14, 26), Some(r("ROM", 16, 27))),
                (r("2ES", 7, 36), None),
            ]
        );
        assert!(parse_mapping("ROM 14:24-26 = ROM 16:25").is_err());
        assert!(parse_mapping("XYZ 1:1 = GEN 1:1").is_err());
        assert!(parse_mapping("ROM 14:24").is_err());
        assert!(parse_mapping("ROM 14:26-24 = ROM 16:27-25").is_err());
    }

    fn romans(last_verse: &str) -> Vec<Book> {
        let src = format!(
            "\\id ROM\n\\c 14\n\\v 23 Sin.\n\\v 24 Now to him.\n\\c 16\n\\v 24 Grace.\n{last_verse}"
        );
        vec![parse_usfm(&src).unwrap()]
    }

    #[test]
    fn rejects_collisions_with_unlisted_verses() {
        let mapping = parse_mapping("ROM 14:24 = ROM 16:25").unwrap();
        // WEB Rom 16:25 holds only a footnote and is dropped, so no collision.
        assert!(check_mapping(&romans("\\v 25 \\f + \\ft TR.\\f*"), &mapping).is_ok());
        // With text, 16:25 maps to itself and collides with 14:24.
        let err = check_mapping(&romans("\\v 25 Amen."), &mapping).unwrap_err();
        assert!(err.to_string().contains("ROM.14.24 and ROM.16.25"), "{err}");
    }

    #[test]
    fn rejects_missing_and_duplicate_verses() {
        let books = romans("");
        assert!(check_mapping(&books, &parse_mapping("ROM 14:99 = ROM 16:25").unwrap()).is_err());
        assert!(
            check_mapping(
                &books,
                &parse_mapping("ROM 14:24 = none\nROM 14:24 = none").unwrap()
            )
            .is_err()
        );
        assert!(check_mapping(&books, &parse_mapping("ROM 14:24 = none").unwrap()).is_ok());
    }
}
