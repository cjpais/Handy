//! English display names for transcription language codes, for the tray menu.
//!
//! The settings window names languages with `Intl.DisplayNames` in the UI
//! language; the tray has no `Intl`, so it uses ISO 639 English names.

use isolang::Language;

/// Names people use that differ from ISO 639's ("Modern Greek", "Pushto").
const OVERRIDES: &[(&str, &str)] = &[
    ("el", "Greek"),
    ("ht", "Haitian Creole"),
    ("nb", "Norwegian"),
    ("pa", "Punjabi"),
    ("ps", "Pashto"),
    ("yue", "Cantonese"),
];

/// A language's English name, from any code a model or setting uses
/// (`en-US`, `jw`). Falls back to the code itself.
pub fn language_name(code: &str) -> String {
    let identity = crate::catalog::language_identity(code);
    if let Some((_, name)) = OVERRIDES.iter().find(|(c, _)| *c == identity) {
        return (*name).to_string();
    }
    let language = match identity.len() {
        2 => Language::from_639_1(&identity),
        3 => Language::from_639_3(&identity),
        _ => None,
    };
    language
        .map(|l| l.to_name().to_string())
        .unwrap_or(identity)
}

#[cfg(test)]
mod tests {
    use super::language_name;

    #[test]
    fn names_codes_however_spelled() {
        assert_eq!(language_name("en-US"), "English");
        assert_eq!(language_name("zh"), "Chinese");
        assert_eq!(language_name("no"), "Norwegian");
        assert_eq!(language_name("yue"), "Cantonese");
        assert_eq!(language_name("jw"), "Javanese");
        assert_eq!(language_name("xx"), "xx");
    }
}
