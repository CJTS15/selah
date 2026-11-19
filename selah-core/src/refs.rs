//! Parsing typed references such as "Jn 3:16", "1 Cor 13" or "Ps 23:1-4".

use crate::store::BookInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reference {
    /// Index into the translation's book list.
    pub book: usize,
    pub chapter: Option<u32>,
    /// First and last verse; equal for a single verse.
    pub verses: Option<(u32, u32)>,
}

// Standard English abbreviations that are not prefixes of the book name.
// ponytail: English only; add per-language tables when the UI is translated.
const ALIASES: &[(&str, &[&str])] = &[
    ("GEN", &["gn"]),
    ("EXO", &["ex"]),
    ("LEV", &["lv"]),
    ("NUM", &["nm", "nb"]),
    ("DEU", &["dt"]),
    ("JOS", &["jsh"]),
    ("JDG", &["jg", "jdg", "jdgs"]),
    ("1SA", &["1sm"]),
    ("2SA", &["2sm"]),
    ("1KI", &["1kgs"]),
    ("2KI", &["2kgs"]),
    ("JOB", &["jb"]),
    ("PSA", &["pss"]),
    ("PRO", &["prv"]),
    ("ECC", &["qoh"]),
    ("SNG", &["song", "sos", "canticles"]),
    ("EZK", &["ezk"]),
    ("DAN", &["dn"]),
    ("JOL", &["jl"]),
    ("OBA", &["ob"]),
    ("JON", &["jnh"]),
    ("NAM", &["na"]),
    ("HAB", &["hb"]),
    ("ZEP", &["zp"]),
    ("HAG", &["hg"]),
    ("ZEC", &["zc"]),
    ("MAL", &["ml"]),
    ("MAT", &["mt"]),
    ("MRK", &["mk"]),
    ("LUK", &["lk"]),
    ("JHN", &["jn"]),
    ("ROM", &["rm"]),
    ("PHP", &["phil", "php"]),
    ("1TH", &["1thess"]),
    ("2TH", &["2thess"]),
    ("1TI", &["1tm"]),
    ("2TI", &["2tm"]),
    ("TIT", &["ti"]),
    ("PHM", &["phlm", "phm"]),
    ("JAS", &["jas", "jm"]),
    ("1PE", &["1pt"]),
    ("2PE", &["2pt"]),
    ("1JN", &["1jn"]),
    ("2JN", &["2jn"]),
    ("3JN", &["3jn"]),
    ("REV", &["rv"]),
];

/// Lowercase with spaces and dots removed: "1 Cor." -> "1cor".
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '.')
        .flat_map(char::to_lowercase)
        .collect()
}

fn find_book(name: &str, books: &[BookInfo]) -> Option<usize> {
    let wanted = normalize(name);
    if wanted.is_empty() {
        return None;
    }
    let exact = books.iter().position(|b| {
        normalize(&b.name) == wanted
            || normalize(&b.abbrev) == wanted
            || b.id.to_lowercase() == wanted
            || ALIASES
                .iter()
                .any(|(id, aliases)| *id == b.id && aliases.contains(&wanted.as_str()))
    });
    if exact.is_some() {
        return exact;
    }
    let mut prefixed = books
        .iter()
        .enumerate()
        .filter(|(_, b)| normalize(&b.name).starts_with(&wanted));
    match (prefixed.next(), prefixed.next()) {
        (Some((i, _)), None) => Some(i),
        _ => None,
    }
}

fn number(s: &str) -> Option<u32> {
    s.trim().parse().ok().filter(|&n| n > 0)
}

/// Parses `Book`, `Book C`, `Book C:V` or `Book C:V-W` against the
/// translation's books. The chapter must exist; verses are checked by the caller.
pub fn parse_reference(input: &str, books: &[BookInfo]) -> Option<Reference> {
    let input = input.trim();
    let split = input.rfind(char::is_alphabetic)?;
    let split = split + input[split..].chars().next()?.len_utf8();
    let (name, numbers) = input.split_at(split);
    let book = find_book(name, books)?;

    // A dot after an abbreviation ("1 Cor. 13") is not a separator.
    let numbers = numbers.trim_start_matches(['.', ' ']).trim();
    if numbers.is_empty() {
        return Some(Reference {
            book,
            chapter: None,
            verses: None,
        });
    }
    let (chapter, verses) = match numbers.split_once([':', '.']) {
        Some((chapter, verses)) => (chapter, Some(verses)),
        None => (numbers, None),
    };
    let chapter = number(chapter)?;
    if !books[book].chapters.contains(&chapter) {
        return None;
    }
    let verses = match verses {
        None => None,
        Some(verses) => Some(match verses.split_once(['-', '–']) {
            Some((first, last)) => {
                let (first, last) = (number(first)?, number(last)?);
                if last < first {
                    return None;
                }
                (first, last)
            }
            None => {
                let verse = number(verses)?;
                (verse, verse)
            }
        }),
    };
    Some(Reference {
        book,
        chapter: Some(chapter),
        verses,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::books::Section;

    fn books() -> Vec<BookInfo> {
        let book = |id: &str, name: &str, abbrev: &str, chapters: u32| BookInfo {
            id: id.into(),
            section: Section::OldTestament,
            name: name.into(),
            abbrev: abbrev.into(),
            chapters: (1..=chapters).collect(),
        };
        vec![
            book("GEN", "Genesis", "Gen", 50),
            book("PSA", "Psalms", "Psa", 150),
            book("SNG", "Song of Solomon", "Sng", 8),
            book("JHN", "John", "Jhn", 21),
            book("1CO", "1 Corinthians", "1Co", 16),
            book("PHP", "Philippians", "Php", 4),
            book("PHM", "Philemon", "Phm", 1),
            book("1JN", "1 John", "1Jn", 5),
            book("JUD", "Jude", "Jud", 1),
        ]
    }

    /// Book id, chapter, verses.
    type Parsed = (String, Option<u32>, Option<(u32, u32)>);

    fn parse(s: &str) -> Option<Parsed> {
        let books = books();
        parse_reference(s, &books).map(|r| (books[r.book].id.clone(), r.chapter, r.verses))
    }

    #[test]
    fn forms() {
        let r = |id: &str, c: Option<u32>, v: Option<(u32, u32)>| Some((id.to_owned(), c, v));
        assert_eq!(parse("Jn 3:16"), r("JHN", Some(3), Some((16, 16))));
        assert_eq!(parse("john 3.16"), r("JHN", Some(3), Some((16, 16))));
        assert_eq!(parse("Jn3:16"), r("JHN", Some(3), Some((16, 16))));
        assert_eq!(parse("1 Cor 13"), r("1CO", Some(13), None));
        assert_eq!(parse("1cor. 13"), r("1CO", Some(13), None));
        assert_eq!(parse("Ps 23:1-4"), r("PSA", Some(23), Some((1, 4))));
        assert_eq!(parse("Psalm 23"), r("PSA", Some(23), None));
        assert_eq!(parse("Gen"), r("GEN", None, None));
        assert_eq!(parse("Song 2"), r("SNG", Some(2), None));
        assert_eq!(parse("1 John 4:8"), r("1JN", Some(4), Some((8, 8))));
        assert_eq!(parse("Phil 4:13"), r("PHP", Some(4), Some((13, 13))));
        assert_eq!(parse("Phlm 1"), r("PHM", Some(1), None));
    }

    #[test]
    fn rejects() {
        assert_eq!(parse("J 3"), None, "ambiguous prefix");
        assert_eq!(parse("Ph 1"), None, "ambiguous prefix");
        assert_eq!(parse("Gen 51"), None, "no such chapter");
        assert_eq!(parse("Ps 23:4-1"), None, "backwards range");
        assert_eq!(parse("Ps 23:x"), None);
        assert_eq!(parse("everlasting life"), None);
        assert_eq!(parse("3:16"), None);
        assert_eq!(parse(""), None);
    }
}
