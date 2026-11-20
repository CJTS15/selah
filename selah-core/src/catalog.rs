//! The translation catalog: openly licensed translations Selah can download.
//!
//! The `selah-catalog` repository publishes `catalog.json`:
//!
//! ```json
//! {"schema": 1, "translations": [{"id": "engbsb", "name": "Berean Standard Bible",
//!   "abbreviation": "BSB", "language": "eng", "language_name": "English",
//!   "direction": "ltr", "licence": "Public Domain", "version": "2026-10-30",
//!   "url": "https://…/engbsb.selah", "size": 3145728, "sha256": "…"}]}
//! ```
//!
//! The catalog comes from the network, so every field is checked here.

use std::collections::HashSet;

use serde::Deserialize;

use crate::Error;
use crate::import::valid_id;

/// The catalog format this version of Selah reads.
pub const SCHEMA: u32 = 1;

/// Largest package the catalog may list; the same cap applies on import.
pub const MAX_SIZE: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub abbreviation: String,
    /// Language code, such as `eng`.
    pub language: String,
    /// The language's own name, such as "Español".
    #[serde(default)]
    pub language_name: String,
    #[serde(default)]
    pub direction: String,
    pub licence: String,
    /// The source text's date, such as `2026-10-30`; newer means an update.
    pub version: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Deserialize)]
struct Catalog {
    schema: u32,
    translations: Vec<Entry>,
}

/// Parses and checks `catalog.json`. Any invalid entry rejects the whole
/// catalog, so a broken publish shows as an error rather than a partial list.
pub fn parse(json: &[u8]) -> Result<Vec<Entry>, Error> {
    let invalid = |message: String| Error::InvalidData(format!("catalog: {message}"));
    let catalog: Catalog = serde_json::from_slice(json).map_err(|err| invalid(err.to_string()))?;
    if catalog.schema != SCHEMA {
        return Err(invalid(format!("unsupported schema {}", catalog.schema)));
    }
    let mut ids = HashSet::new();
    for entry in &catalog.translations {
        let problem = if !valid_id(&entry.id) {
            Some("bad id")
        } else if !ids.insert(entry.id.as_str()) {
            Some("duplicate id")
        } else if entry.name.trim().is_empty() || entry.licence.trim().is_empty() {
            Some("missing name or licence")
        } else if !entry.url.starts_with("https://") {
            Some("URL is not HTTPS")
        } else if entry.size == 0 || entry.size > MAX_SIZE {
            Some("bad size")
        } else if entry.sha256.len() != 64
            || !entry
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            Some("bad sha256")
        } else {
            None
        };
        if let Some(problem) = problem {
            return Err(invalid(format!("{problem} in entry {:?}", entry.id)));
        }
    }
    Ok(catalog.translations)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn entry() -> Value {
        json!({
            "id": "engbsb", "name": "Berean Standard Bible", "language": "eng",
            "licence": "Public Domain", "version": "2026-10-30",
            "url": "https://example.org/engbsb.selah", "size": 3145728,
            "sha256": "ab".repeat(32), "abbreviation": "BSB", "extra": 1
        })
    }

    fn catalog(entries: Vec<Value>) -> Vec<u8> {
        json!({"schema": 1, "generated": "x", "translations": entries})
            .to_string()
            .into_bytes()
    }

    #[test]
    fn parses_entries() {
        let parsed = parse(&catalog(vec![entry()])).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].abbreviation, "BSB");
        assert_eq!(parsed[0].language_name, "", "optional");
        assert_eq!(parse(&catalog(Vec::new())).unwrap(), Vec::new());
    }

    #[test]
    fn rejects_bad_catalogs() {
        let message = |json: Vec<u8>| parse(&json).unwrap_err().to_string();
        assert!(message(b"not json".to_vec()).contains("catalog"));
        assert!(message(br#"{"schema": 2, "translations": []}"#.to_vec()).contains("schema"));
        assert!(message(catalog(vec![entry(), entry()])).contains("duplicate id"));
        assert!(
            message(br#"{"schema": 1, "translations": [{"id": "x"}]}"#.to_vec())
                .contains("missing field")
        );
        for (field, value, expected) in [
            ("id", json!("../x"), "bad id"),
            ("name", json!("  "), "missing name"),
            ("licence", json!(""), "missing name"),
            ("url", json!("http://example.org/x.selah"), "not HTTPS"),
            ("size", json!(0), "bad size"),
            ("size", json!(MAX_SIZE + 1), "bad size"),
            ("sha256", json!("AB".repeat(32)), "bad sha256"),
            ("sha256", json!("ab"), "bad sha256"),
        ] {
            let mut bad = entry();
            bad[field] = value;
            let got = message(catalog(vec![bad]));
            assert!(got.contains(expected), "{field}: {got}");
        }
    }
}
