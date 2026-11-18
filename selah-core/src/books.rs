//! Canonical book identifiers and the section each belongs to.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    OldTestament,
    NewTestament,
    Deuterocanon,
}

impl Section {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OldTestament => "OT",
            Self::NewTestament => "NT",
            Self::Deuterocanon => "DC",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "OT" => Some(Self::OldTestament),
            "NT" => Some(Self::NewTestament),
            "DC" => Some(Self::Deuterocanon),
            _ => None,
        }
    }
}

// USFM 3 book identifiers, in the order of the USFM specification
// (https://ubsicap.github.io/usfm/identification/books.html). Front and back
// matter (FRT, GLO, ...) is not scripture and is left out.
const OLD_TESTAMENT: &[&str] = &[
    "GEN", "EXO", "LEV", "NUM", "DEU", "JOS", "JDG", "RUT", "1SA", "2SA", "1KI", "2KI", "1CH",
    "2CH", "EZR", "NEH", "EST", "JOB", "PSA", "PRO", "ECC", "SNG", "ISA", "JER", "LAM", "EZK",
    "DAN", "HOS", "JOL", "AMO", "OBA", "JON", "MIC", "NAM", "HAB", "ZEP", "HAG", "ZEC", "MAL",
];

const NEW_TESTAMENT: &[&str] = &[
    "MAT", "MRK", "LUK", "JHN", "ACT", "ROM", "1CO", "2CO", "GAL", "EPH", "PHP", "COL", "1TH",
    "2TH", "1TI", "2TI", "TIT", "PHM", "HEB", "JAS", "1PE", "2PE", "1JN", "2JN", "3JN", "JUD",
    "REV",
];

// Deuterocanonical books plus the additional books some traditions include.
const DEUTEROCANON: &[&str] = &[
    "TOB", "JDT", "ESG", "WIS", "SIR", "BAR", "LJE", "S3Y", "SUS", "BEL", "1MA", "2MA", "3MA",
    "4MA", "1ES", "2ES", "MAN", "PS2", "ODA", "PSS", "EZA", "5EZ", "6EZ", "DAG", "PS3", "2BA",
    "LBA", "JUB", "ENO", "1MQ", "2MQ", "3MQ", "REP", "4BA", "LAO",
];

/// The section of a canonical book, or `None` for an unknown identifier.
pub fn section(id: &str) -> Option<Section> {
    if OLD_TESTAMENT.contains(&id) {
        Some(Section::OldTestament)
    } else if NEW_TESTAMENT.contains(&id) {
        Some(Section::NewTestament)
    } else if DEUTEROCANON.contains(&id) {
        Some(Section::Deuterocanon)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections() {
        assert_eq!(OLD_TESTAMENT.len(), 39);
        assert_eq!(NEW_TESTAMENT.len(), 27);
        assert_eq!(section("GEN"), Some(Section::OldTestament));
        assert_eq!(section("JHN"), Some(Section::NewTestament));
        assert_eq!(section("TOB"), Some(Section::Deuterocanon));
        assert_eq!(section("FRT"), None);
        assert_eq!(section("jhn"), None);
    }

    #[test]
    fn section_round_trip() {
        for s in [
            Section::OldTestament,
            Section::NewTestament,
            Section::Deuterocanon,
        ] {
            assert_eq!(Section::parse(s.as_str()), Some(s));
        }
    }
}
