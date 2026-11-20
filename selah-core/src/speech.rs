//! Read aloud support that needs no audio: which espeak-ng voice speaks a
//! translation's language, and making espeak-ng's streamed WAV playable.

/// ISO 639-3 (as in translation metadata) or 639-1 codes, and the espeak-ng
/// voice for each. Languages missing here have no espeak-ng voice; Tagalog
/// (`tgl`) is one of them in espeak-ng 1.52.
const VOICES: &[(&str, &str)] = &[
    ("afr", "af"),
    ("amh", "am"),
    ("ara", "ar"),
    ("aze", "az"),
    ("ben", "bn"),
    ("bul", "bg"),
    ("cat", "ca"),
    ("ces", "cs"),
    ("cmn", "cmn"),
    ("cym", "cy"),
    ("dan", "da"),
    ("deu", "de"),
    ("ell", "el"),
    ("eng", "en"),
    ("epo", "eo"),
    ("est", "et"),
    ("eus", "eu"),
    ("fas", "fa"),
    ("fin", "fi"),
    ("fra", "fr"),
    ("gle", "ga"),
    ("guj", "gu"),
    ("heb", "he"),
    ("hin", "hi"),
    ("hrv", "hr"),
    ("hun", "hu"),
    ("hye", "hy"),
    ("ind", "id"),
    ("isl", "is"),
    ("ita", "it"),
    ("jpn", "ja"),
    ("kan", "kn"),
    ("kat", "ka"),
    ("kaz", "kk"),
    ("kor", "ko"),
    ("lat", "la"),
    ("lav", "lv"),
    ("lit", "lt"),
    ("mal", "ml"),
    ("mar", "mr"),
    ("msa", "ms"),
    ("mya", "my"),
    ("nep", "ne"),
    ("nld", "nl"),
    ("nob", "nb"),
    ("nor", "nb"),
    ("pan", "pa"),
    ("pol", "pl"),
    ("por", "pt"),
    ("ron", "ro"),
    ("rus", "ru"),
    ("sin", "si"),
    ("slk", "sk"),
    ("slv", "sl"),
    ("spa", "es"),
    ("sqi", "sq"),
    ("srp", "sr"),
    ("swa", "sw"),
    ("swe", "sv"),
    ("swh", "sw"),
    ("tam", "ta"),
    ("tel", "te"),
    ("tur", "tr"),
    ("ukr", "uk"),
    ("urd", "ur"),
    ("uzb", "uz"),
    ("vie", "vi"),
    ("yue", "yue"),
    ("zsm", "ms"),
];

/// The espeak-ng voice for a language code, if espeak-ng can speak it.
/// Accepts ISO 639-3 (`spa`), 639-1 (`es`) and region tags (`en-US`).
pub fn voice_for_language(code: &str) -> Option<&'static str> {
    let base = code.split(['-', '_']).next()?.to_ascii_lowercase();
    VOICES
        .iter()
        .find(|(iso3, voice)| *iso3 == base || *voice == base)
        .map(|(_, voice)| *voice)
}

/// Words per minute for the Speed preference (`slower`, `normal`, `faster`).
pub fn words_per_minute(speed: &str) -> u32 {
    match speed {
        "slower" => 130,
        "faster" => 200,
        _ => 165,
    }
}

/// espeak-ng streams WAV with placeholder sizes; writes the real RIFF and
/// data chunk sizes so players see the true length. Returns false for data
/// that is not a canonical 44-byte-header WAV, which is then left unchanged.
pub fn fix_wav_sizes(wav: &mut [u8]) -> bool {
    if wav.len() < 44 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" || &wav[36..40] != b"data"
    {
        return false;
    }
    let (Ok(riff), Ok(data)) = (u32::try_from(wav.len() - 8), u32::try_from(wav.len() - 44)) else {
        return false;
    };
    wav[4..8].copy_from_slice(&riff.to_le_bytes());
    wav[40..44].copy_from_slice(&data.to_le_bytes());
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voices_by_language() {
        assert_eq!(voice_for_language("eng"), Some("en"));
        assert_eq!(voice_for_language("spa"), Some("es"));
        assert_eq!(voice_for_language("deu"), Some("de"));
        assert_eq!(voice_for_language("es"), Some("es"));
        assert_eq!(voice_for_language("en-US"), Some("en"));
        assert_eq!(voice_for_language("tgl"), None, "no Tagalog voice");
        assert_eq!(voice_for_language(""), None);
    }

    #[test]
    fn fixes_streamed_wav_sizes() {
        let mut wav = b"RIFF\xff\xff\xff\x7fWAVEfmt ".to_vec();
        wav.extend_from_slice(&[0; 20]); // the rest of the fmt chunk
        wav.extend_from_slice(b"data\xff\xff\xff\x7f");
        wav.extend_from_slice(&[1, 2, 3, 4]);
        assert!(fix_wav_sizes(&mut wav));
        assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()), 40);
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 4);
        assert!(!fix_wav_sizes(
            &mut b"not a wav file at all, but long enough to pass".to_vec()
        ));
    }

    #[test]
    fn speeds() {
        assert!(words_per_minute("slower") < words_per_minute("normal"));
        assert!(words_per_minute("faster") > words_per_minute("normal"));
        assert_eq!(words_per_minute("unknown"), words_per_minute("normal"));
    }
}
