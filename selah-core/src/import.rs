//! USFM import.
//!
//! Keeps verse text, paragraph starts and headings. Drops footnotes, cross
//! references, character styling and word attributes such as Strong's numbers.
//! All text is stored plain.

use std::fs;
use std::path::Path;

use crate::Error;
use crate::books::{self, Section};
use crate::versification::{self, Mapping};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Book {
    pub id: String,
    pub section: Section,
    pub name: String,
    pub abbrev: String,
    pub verses: Vec<Verse>,
    pub headings: Vec<Heading>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verse {
    pub chapter: u32,
    pub verse: u32,
    /// Last verse of a combined range such as `\v 15-16`.
    pub verse_end: Option<u32>,
    /// The verse starts a new paragraph or poetry line.
    pub para: bool,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadingKind {
    /// Descriptive title, such as a Psalm title (`\d`).
    Title,
    /// Section heading (`\s`, `\ms`).
    Section,
    /// Paragraph text outside any verse, such as the Prologue to Sirach.
    Text,
}

impl HeadingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Section => "section",
            Self::Text => "text",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "title" => Some(Self::Title),
            "section" => Some(Self::Section),
            "text" => Some(Self::Text),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub chapter: u32,
    /// Shown before this verse. One past the last verse means after the chapter.
    pub before_verse: u32,
    pub kind: HeadingKind,
    pub text: String,
}

/// Translation metadata as `key=value` pairs, in file order.
pub type Meta = Vec<(String, String)>;

const REQUIRED_META: &[&str] = &["id", "name", "language", "direction", "licence"];

/// Reads `meta.txt`, every `*.usfm` file and the optional `versification.txt`
/// in `dir`. Books keep the order of the file names, which is the
/// translation's own book order.
pub fn import_dir(dir: &Path) -> Result<(Meta, Vec<Book>, Mapping), Error> {
    let meta = parse_meta(&fs::read_to_string(dir.join("meta.txt"))?)?;

    let mut paths = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "usfm") {
            paths.push(path);
        }
    }
    paths.sort();

    let mut books = Vec::with_capacity(paths.len());
    for path in paths {
        let src = fs::read_to_string(&path)?;
        // Front matter, introductions, glossaries and the like are not scripture.
        if book_id(&src).is_some_and(books::is_peripheral) {
            continue;
        }
        let book = parse_usfm(&src).map_err(|err| match err {
            Error::Usfm { line, message } => Error::Usfm {
                line,
                message: format!("{}: {message}", path.display()),
            },
            other => other,
        })?;
        books.push(book);
    }

    let mapping = match fs::read_to_string(dir.join("versification.txt")) {
        Ok(src) => versification::parse_mapping(&src)?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Mapping::new(),
        Err(err) => return Err(err.into()),
    };
    versification::check_mapping(&books, &mapping)?;
    Ok((meta, books, mapping))
}

/// The id after `\id`, as the parser reads it.
fn book_id(src: &str) -> Option<&str> {
    let at = src.find("\\id")?;
    src[at + 3..].split_whitespace().next()
}

pub fn parse_meta(src: &str) -> Result<Meta, Error> {
    let mut meta = Meta::new();
    for line in src.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| Error::Meta(format!("expected key=value, found {line:?}")))?;
        meta.push((key.trim().to_owned(), value.trim().to_owned()));
    }
    check_meta(&meta)?;
    Ok(meta)
}

/// Requires the keys every translation needs and a usable id.
pub fn check_meta(meta: &Meta) -> Result<(), Error> {
    for key in REQUIRED_META {
        if !meta.iter().any(|(k, _)| k == key) {
            return Err(Error::Meta(format!("missing {key}")));
        }
    }
    let id = meta
        .iter()
        .find(|(k, _)| k == "id")
        .map(|(_, v)| v.as_str());
    if !id.is_some_and(valid_id) {
        return Err(Error::Meta(format!("unusable id {id:?}")));
    }
    Ok(())
}

/// Ids name files, so only letters, digits, `-` and `_` are allowed.
pub fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    /// Book-level material before the first chapter (titles, introductions).
    Ignore,
    /// Inside a chapter but outside a verse or heading.
    Loose,
    Verse,
    Heading,
}

struct Parser {
    id: Option<String>,
    name: Option<String>,
    short_name: Option<String>,
    abbrev: Option<String>,
    chapter: u32,
    last_verse: u32,
    pending_para: bool,
    target: Target,
    /// Inside a footnote or cross reference.
    skipping: bool,
    verses: Vec<Verse>,
    headings: Vec<Heading>,
}

pub fn parse_usfm(src: &str) -> Result<Book, Error> {
    let mut parser = Parser {
        id: None,
        name: None,
        short_name: None,
        abbrev: None,
        chapter: 0,
        last_verse: 0,
        pending_para: false,
        target: Target::Ignore,
        skipping: false,
        verses: Vec::new(),
        headings: Vec::new(),
    };

    // Text before the first marker is not part of any element.
    let mut pieces = src.split('\\');
    let mut offset = pieces.next().map_or(0, str::len);
    for piece in pieces {
        parser.marker(piece).map_err(|message| Error::Usfm {
            line: src[..offset].matches('\n').count() + 1,
            message,
        })?;
        offset += piece.len() + 1;
    }
    parser.finish()
}

impl Parser {
    fn marker(&mut self, piece: &str) -> Result<(), String> {
        let end = piece
            .find(|c: char| c.is_whitespace() || c == '*')
            .map_or(piece.len(), |i| {
                if piece[i..].starts_with('*') {
                    i + 1
                } else {
                    i
                }
            });
        let (marker, mut rest) = piece.split_at(end);
        // One whitespace character after an opening marker separates it from its content.
        if !marker.ends_with('*') {
            let mut chars = rest.chars();
            if chars.next().is_some_and(char::is_whitespace) {
                rest = chars.as_str();
            }
        }

        if self.skipping {
            if matches!(marker, "f*" | "fe*" | "x*") {
                self.skipping = false;
                self.append(rest);
            }
            return Ok(());
        }

        match marker {
            "id" => self.id = rest.split_whitespace().next().map(str::to_owned),
            "toc2" => self.name = Some(collapse(rest)),
            "toc3" => self.abbrev = Some(collapse(rest)),
            "h" => self.short_name = Some(collapse(rest)),
            "ide" | "toc1" | "mt" | "mt1" | "mt2" | "mt3" | "imt" | "imt1" | "is" | "is1"
            | "ip" | "rem" | "usfm" | "sts" => self.target = Target::Ignore,
            // Chapter labels (`\cl Psalm`, `\cp 151`) repeat the chapter number.
            // Speaker labels (`\sp Beloved`, Song of Songs) are editorial and
            // often fall mid-verse, so they are dropped too.
            "cl" | "cp" | "sp" => {}
            // Parallel passage references under headings (`\r (Mark 1:1-8)`).
            "r" | "mr" => self.target = Target::Ignore,
            "c" => {
                self.end_chapter();
                self.chapter = parse_number(rest.trim(), "chapter")?;
                self.last_verse = 0;
                self.target = Target::Loose;
            }
            "v" => {
                if self.chapter == 0 {
                    return Err("verse before the first chapter".into());
                }
                let rest = rest.trim_start();
                let (number, text) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                let (verse, verse_end) = match number.split_once('-') {
                    Some((first, last)) => {
                        let (first, last) =
                            (parse_number(first, "verse")?, parse_number(last, "verse")?);
                        if last <= first {
                            return Err(format!("unsupported verse range {number:?}"));
                        }
                        (first, Some(last))
                    }
                    None => (parse_number(number, "verse")?, None),
                };
                for heading in &mut self.headings {
                    if heading.before_verse == 0 {
                        heading.before_verse = verse;
                    }
                }
                self.verses.push(Verse {
                    chapter: self.chapter,
                    verse,
                    verse_end,
                    para: std::mem::take(&mut self.pending_para),
                    text: String::new(),
                });
                self.last_verse = verse_end.unwrap_or(verse);
                self.target = Target::Verse;
                self.append(text);
            }
            "p" | "m" | "b" | "nb" | "pi" | "pi1" | "pi2" | "mi" | "pc" | "q" | "q1" | "q2"
            | "q3" | "li" | "li1" | "li2" | "pmo" | "qr" | "qc" => {
                self.pending_para = true;
                if self.target == Target::Heading
                    || (self.target == Target::Ignore && self.chapter > 0)
                {
                    self.target = Target::Loose;
                }
                self.append(rest);
            }
            "d" => self.start_heading(HeadingKind::Title, rest),
            // `\qa` is an acrostic heading such as "Aleph" in Psalm 119.
            "s" | "s1" | "s2" | "s3" | "ms" | "ms1" | "ms2" | "qa" => {
                self.start_heading(HeadingKind::Section, rest)
            }
            "f" | "fe" | "x" => self.skipping = true,
            // Character markers and their end markers: keep the text.
            _ => self.append(rest),
        }
        Ok(())
    }

    fn start_heading(&mut self, kind: HeadingKind, text: &str) {
        if self.chapter == 0 {
            self.target = Target::Ignore;
            return;
        }
        self.headings.push(Heading {
            chapter: self.chapter,
            before_verse: 0,
            kind,
            text: String::new(),
        });
        self.target = Target::Heading;
        self.append(text);
    }

    fn append(&mut self, text: &str) {
        // Word attributes follow `|` up to the closing marker: `\w word|strong="H1"\w*`.
        let text = text.split('|').next().unwrap_or_default();
        match self.target {
            Target::Ignore => {}
            Target::Verse => {
                if let Some(verse) = self.verses.last_mut() {
                    verse.text.push_str(text);
                }
            }
            Target::Heading => {
                if let Some(heading) = self.headings.last_mut() {
                    heading.text.push_str(text);
                }
            }
            Target::Loose => {
                if !text.trim().is_empty() {
                    self.start_heading(HeadingKind::Text, text);
                }
            }
        }
    }

    /// Headings still waiting for a verse go after the chapter's last verse.
    fn end_chapter(&mut self) {
        for heading in &mut self.headings {
            if heading.before_verse == 0 {
                heading.before_verse = self.last_verse + 1;
            }
        }
    }

    fn finish(mut self) -> Result<Book, Error> {
        self.end_chapter();
        let error = |message: String| Error::Usfm { line: 1, message };
        let id = self.id.ok_or_else(|| error("missing \\id".into()))?;
        let section =
            books::section(&id).ok_or_else(|| error(format!("unknown book id {id:?}")))?;
        let name = self.name.or(self.short_name).unwrap_or_else(|| id.clone());
        let abbrev = self.abbrev.unwrap_or_else(|| name.clone());

        // `¶` marks (KJV) duplicate the paragraph breaks already kept in `para`.
        for verse in &mut self.verses {
            verse.text = collapse(&verse.text.replace('¶', ""));
        }
        for heading in &mut self.headings {
            heading.text = collapse(&heading.text.replace('¶', ""));
        }
        // A verse holding only a footnote (WEB Rom 16:25) is not text; its
        // paragraph start moves to the next verse.
        let mut carry_para = false;
        self.verses.retain_mut(|verse| {
            verse.para |= std::mem::take(&mut carry_para);
            carry_para = verse.text.is_empty() && verse.para;
            !verse.text.is_empty()
        });
        self.headings.retain(|heading| !heading.text.is_empty());

        Ok(Book {
            id,
            section,
            name,
            abbrev,
            verses: self.verses,
            headings: self.headings,
        })
    }
}

fn parse_number(s: &str, what: &str) -> Result<u32, String> {
    s.parse::<u32>()
        .ok()
        .filter(|&n| n > 0)
        .ok_or_else(|| format!("unsupported {what} number {s:?}"))
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PSALM: &str = r#"\id PSA The Book of Psalms
\h Psalms
\toc2 Psalms
\toc3 Psa
\mt1 THE BOOK OF PSALMS
\c 3
\d A \w Psalm|strong="H4210"\w* of \w David|strong="H1732"\w*, when he fled.
\q1
\v 1 \nd \+w LORD|strong="H3068"\+w*\nd*, how are they \w increased|strong="H7231"\w*! many \add are\add* they.
\q1
\v 2 ¶ Many \add there be\add* which say\f + \fr 3.2 \ft Or, of my soul\f*.
"#;

    #[test]
    fn strips_markup_and_keeps_titles() {
        let book = parse_usfm(PSALM).unwrap();
        assert_eq!(book.id, "PSA");
        assert_eq!(book.section, Section::OldTestament);
        assert_eq!(book.name, "Psalms");
        assert_eq!(book.abbrev, "Psa");
        assert_eq!(
            book.verses,
            vec![
                Verse {
                    chapter: 3,
                    verse: 1,
                    verse_end: None,
                    para: true,
                    text: "LORD, how are they increased! many are they.".into(),
                },
                Verse {
                    chapter: 3,
                    verse: 2,
                    verse_end: None,
                    para: true,
                    text: "Many there be which say.".into(),
                },
            ]
        );
        assert_eq!(
            book.headings,
            vec![Heading {
                chapter: 3,
                before_verse: 1,
                kind: HeadingKind::Title,
                text: "A Psalm of David, when he fled.".into(),
            }]
        );
    }

    #[test]
    fn prologue_and_trailing_heading() {
        let src = r"\id SIR
\c 1
\s1 A Prologue
\p This Jesus was the son of Sirach.
\p
\v 1 All wisdom cometh from the Lord.
\v 2 Who can number the sand?
\s1 Written from Rome.
";
        let book = parse_usfm(src).unwrap();
        let headings: Vec<_> = book
            .headings
            .iter()
            .map(|h| (h.before_verse, h.kind, h.text.as_str()))
            .collect();
        assert_eq!(
            headings,
            vec![
                (1, HeadingKind::Section, "A Prologue"),
                (1, HeadingKind::Text, "This Jesus was the son of Sirach."),
                (3, HeadingKind::Section, "Written from Rome."),
            ]
        );
        assert!(book.verses[0].para);
        assert!(!book.verses[1].para);
    }

    #[test]
    fn chapter_may_start_after_verse_one() {
        let book = parse_usfm("\\id ESG\n\\c 10\n\\p\n\\v 4 Then Mardocheus said.\n").unwrap();
        assert_eq!(book.verses[0].verse, 4);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(matches!(
            parse_usfm("\\id XYZ\n\\c 1\n\\v 1 x"),
            Err(Error::Usfm { .. })
        ));
        assert!(matches!(
            parse_usfm("\\id GEN\n\\c 1\n\\v 2-1 x"),
            Err(Error::Usfm { line: 3, .. })
        ));
        assert!(parse_usfm("\\id GEN\n\\c 1\n\\v 1a x").is_err());
        assert!(parse_usfm("\\id GEN\n\\v 1 x").is_err());
        assert!(parse_usfm("\\c 1\n\\v 1 x").is_err());
    }

    #[test]
    fn ranges_empty_verses_and_labels() {
        let src = r"\id SIR
\c 11
\cl Chapter
\cp XI
\sp Beloved
\p
\v 14 Good things and bad.
\sp Friends
\q1 Rejoice.
\p
\v 15-16 \f + \fr 11:15-16 \ft Omitted by the best authorities. \f*
\v 17 The gift of the Lord.
\v 18-19 So they said.
";
        let book = parse_usfm(src).unwrap();
        let verses: Vec<_> = book
            .verses
            .iter()
            .map(|v| (v.verse, v.verse_end, v.para, v.text.as_str()))
            .collect();
        assert_eq!(
            verses,
            vec![
                (14, None, true, "Good things and bad. Rejoice."),
                (17, None, true, "The gift of the Lord."),
                (18, Some(19), false, "So they said."),
            ]
        );
        assert!(book.headings.is_empty(), "{:?}", book.headings);
    }

    #[test]
    fn references_and_poetry_markers() {
        let src = r"\id PSA
\c 119
\s1 Your Word Is a Lamp
\r (John 1:1-5)
\qa Aleph
\q1
\v 1 Blessed are they
\qr whose way is blameless.
\pmo
\v 2 Blessed.
";
        let book = parse_usfm(src).unwrap();
        let headings: Vec<_> = book.headings.iter().map(|h| h.text.as_str()).collect();
        assert_eq!(headings, ["Your Word Is a Lamp", "Aleph"]);
        assert_eq!(
            book.verses[0].text,
            "Blessed are they whose way is blameless."
        );
        assert!(book.verses[1].para);
    }

    #[test]
    fn meta_requires_keys() {
        let meta =
            parse_meta("# comment\nid=eng-kjv\nname=KJV\nlanguage=en\ndirection=ltr\nlicence=PD\n")
                .unwrap();
        assert_eq!(meta[0], ("id".into(), "eng-kjv".into()));
        assert!(parse_meta("id=x\n").is_err());
        assert!(parse_meta("id=../x\nname=x\nlanguage=en\ndirection=ltr\nlicence=PD\n").is_err());
        assert!(valid_id("eng-kjv") && valid_id("eng_web2"));
        assert!(
            !valid_id("") && !valid_id("a/b") && !valid_id("a.b") && !valid_id(&"x".repeat(65))
        );
        assert!(parse_meta("nonsense\n").is_err());
    }
}
