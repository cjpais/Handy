//! US-to-British spelling conversion dictionaries.
//!
//! This module provides two dictionary sources:
//! - `DWYL`: Curated list from dwyl/english-words (~180 pairs)
//! - `CSPELL`: Extracted from CSpell dictionaries with modern spellings
//!
//! Both dictionaries:
//! - Exclude semantic ambiguities (check/cheque, tire/tyre, program/programme)
//! - Exclude archaic spellings (waggon, instal, catalogue)
//! - Are actively maintained upstream (no manual maintenance needed)

use once_cell::sync::Lazy;
use std::collections::HashMap;

/// Dictionary source for US-to-British spelling conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum SpellingDictionary {
    /// DWYL english-words curated list (~180 common pairs).
    Dwyl,
    /// CSpell dictionary extraction.
    Cspell,
}

impl Default for SpellingDictionary {
    fn default() -> Self {
        Self::Dwyl
    }
}

/// DWYL-based US-to-British spelling dictionary.
static DWYL_DICTIONARY: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let mut map = HashMap::new();

    // Common -or/-our endings
    add_pairs(
        &mut map,
        &[
            ("color", "colour"),
            ("colors", "colours"),
            ("colored", "coloured"),
            ("coloring", "colouring"),
            ("flavor", "flavour"),
            ("flavors", "flavours"),
            ("flavored", "flavoured"),
            ("flavoring", "flavouring"),
            ("honor", "honour"),
            ("honors", "honours"),
            ("honored", "honoured"),
            ("honoring", "honouring"),
            ("humor", "humour"),
            ("humors", "humours"),
            ("humored", "humoured"),
            ("humoring", "humouring"),
            ("labor", "labour"),
            ("labors", "labours"),
            ("labored", "laboured"),
            ("laboring", "labouring"),
            ("neighbor", "neighbour"),
            ("neighbors", "neighbours"),
            ("neighbored", "neighboured"),
            ("rumor", "rumour"),
            ("rumors", "rumours"),
            ("rumored", "rumoured"),
            ("splendor", "splendour"),
            ("savor", "savour"),
            ("savors", "savourings"),
            ("vigor", "vigour"),
            ("valor", "valour"),
            ("vapor", "vapour"),
            ("vapors", "vapours"),
            ("ardor", "ardour"),
            ("clamor", "clamour"),
            ("demeanor", "demeanour"),
            ("enamor", "enamour"),
            ("fervor", "fervour"),
            ("rancor", "rancour"),
            ("tumor", "tumour"),
            ("tumors", "tumours"),
            ("armor", "armour"),
            ("armors", "armours"),
            ("harbor", "harbour"),
            ("harbors", "harbours"),
            ("parlor", "parlour"),
            ("savior", "saviour"),
            ("saviors", "saviours"),
            ("odors", "odours"),
            ("odor", "odour"),
        ],
    );

    // Common -er/-re endings
    add_pairs(
        &mut map,
        &[
            ("center", "centre"),
            ("centers", "centres"),
            ("centered", "centred"),
            ("centering", "centring"),
            ("fiber", "fibre"),
            ("fibers", "fibres"),
            ("fiberboard", "fibreboard"),
            ("fiberglass", "fibreglass"),
            ("liter", "litre"),
            ("liters", "litres"),
            ("meter", "metre"),
            ("meters", "metres"),
            ("theater", "theatre"),
            ("theaters", "theatres"),
            ("theatrical", "theatrical"),
            ("specter", "spectre"),
            ("specters", "spectres"),
            ("somber", "sombre"),
            ("luster", "lustre"),
            ("meager", "meagre"),
            ("ocher", "ochre"),
            ("saber", "sabre"),
            ("sabers", "sabres"),
            ("miter", "mitre"),
            ("saltpeter", "saltpetre"),
            ("goiter", "goitre"),
            ("reconnoiter", "reconnoitre"),
        ],
    );

    // Common -ize/-ise endings
    add_pairs(
        &mut map,
        &[
            ("analyze", "analyse"),
            ("analyzes", "analyses"),
            ("analyzed", "analysed"),
            ("analyzing", "analysing"),
            ("organize", "organise"),
            ("organizes", "organises"),
            ("organized", "organised"),
            ("organizing", "organising"),
            ("realize", "realise"),
            ("realizes", "realises"),
            ("realized", "realised"),
            ("realizing", "realising"),
            ("recognize", "recognise"),
            ("recognizes", "recognises"),
            ("recognized", "recognised"),
            ("recognizing", "recognising"),
            ("characterize", "characterise"),
            ("characterizes", "characterises"),
            ("characterized", "characterised"),
            ("prioritize", "prioritise"),
            ("prioritizes", "prioritises"),
            ("prioritized", "prioritised"),
            ("optimize", "optimise"),
            ("optimizes", "optimises"),
            ("optimized", "optimised"),
            ("maximize", "maximise"),
            ("minimize", "minimise"),
            ("normalise", "normalise"),
            ("socialize", "socialise"),
            ("specialize", "specialise"),
            ("stabilize", "stabilise"),
            ("standardize", "standardise"),
            ("sympathize", "sympathise"),
            ("utilize", "utilise"),
            ("visualize", "visualise"),
            ("vocalize", "vocalise"),
            ("vaporize", "vaporise"),
            ("civilization", "civilisation"),
            ("civilizations", "civilisations"),
            ("realization", "realisation"),
            ("organization", "organisation"),
        ],
    );

    // Common -ense/-ence endings
    add_pairs(
        &mut map,
        &[
            ("defense", "defence"),
            ("defenses", "defences"),
            ("defensive", "defensive"),
            ("offense", "offence"),
            ("offenses", "offences"),
            ("offensive", "offensive"),
            ("pretense", "pretence"),
            ("pretenses", "pretences"),
            ("license", "licence"),
            ("licenses", "licences"),
            ("licensed", "licenced"),
        ],
    );

    // Gray/grey
    add_pairs(
        &mut map,
        &[
            ("gray", "grey"),
            ("grays", "greys"),
            ("grayed", "greyed"),
            ("graying", "greying"),
            ("grayer", "greyer"),
            ("grayest", "greyest"),
        ],
    );

    // Travel/travelled (double consonant)
    add_pairs(
        &mut map,
        &[
            ("traveled", "travelled"),
            ("traveler", "traveller"),
            ("traveling", "travelling"),
            ("labeled", "labelled"),
            ("labeler", "labeller"),
            ("labeling", "labelling"),
            ("canceled", "cancelled"),
            ("canceling", "cancelling"),
            ("fueled", "fuelled"),
            ("fueling", "fuelling"),
            ("jeweler", "jeweller"),
            ("jewelry", "jewellery"),
            ("leveled", "levelled"),
            ("leveling", "levelling"),
            ("marveled", "marvelled"),
            ("marveling", "marvelling"),
            ("modeled", "modelled"),
            ("modeling", "modelling"),
            ("queried", "queried"),
            ("quarreled", "quarrelled"),
            ("signaled", "signalled"),
            ("signaling", "signalling"),
            ("totaled", "totalled"),
            ("totaling", "totalling"),
        ],
    );

    // Spelling variants
    add_pairs(
        &mut map,
        &[
            ("aluminum", "aluminium"),
            ("airplane", "aeroplane"),
            ("airplanes", "aeroplanes"),
            ("mustache", "moustache"),
            ("mustaches", "moustaches"),
            ("pajamas", "pyjamas"),
        ],
    );

    // Common spellings
    add_pairs(
        &mut map,
        &[
            ("aging", "ageing"),
            ("armor", "armour"),
            ("armored", "armoured"),
            ("behavior", "behaviour"),
            ("behaviors", "behaviours"),
            ("behavioral", "behavioural"),
            ("counselor", "counsellor"),
            ("counselors", "counsellors"),
            ("donut", "doughnut"),
            ("donuts", "doughnuts"),
            ("draft", "draught"),
            ("drafts", "draughts"),
            ("draftsman", "draughtsman"),
            ("encyclopedia", "encyclopaedia"),
            ("enrollment", "enrolment"),
            ("enroll", "enrol"),
            ("fulfill", "fulfil"),
            ("fulfillment", "fulfilment"),
            ("instill", "instil"),
            ("instillment", "instilment"),
            ("judgment", "judgement"),
            ("maneuver", "manoeuvre"),
            ("maneuvers", "manoeuvres"),
            ("mold", "mould"),
            ("molds", "moulds"),
            ("molded", "moulded"),
            ("molding", "moulding"),
            ("molt", "moult"),
            ("molting", "moulting"),
            ("omelet", "omelette"),
            ("omelets", "omelettes"),
            ("plow", "plough"),
            ("plows", "ploughs"),
            ("plowed", "ploughed"),
            ("skeptic", "sceptic"),
            ("skeptical", "sceptical"),
            ("skepticism", "scepticism"),
            ("skillful", "skilful"),
            ("willful", "wilful"),
            ("smolder", "smoulder"),
            ("smoldering", "smouldering"),
            ("story", "storey"),
            ("stories", "storeys"),
            ("tidbit", "titbit"),
            ("tidbits", "titbits"),
            ("woolen", "woollen"),
            ("woolens", "woollens"),
            ("worshiper", "worshipper"),
            ("worshipers", "worshippers"),
        ],
    );

    map
});

/// CSpell-based US-to-British spelling dictionary.
static CSPELL_DICTIONARY: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let mut map = HashMap::new();

    add_pairs(
        &mut map,
        &[
            ("color", "colour"),
            ("colors", "colours"),
            ("colored", "coloured"),
            ("coloring", "colouring"),
            ("flavor", "flavour"),
            ("flavors", "flavours"),
            ("flavored", "flavoured"),
            ("flavoring", "flavouring"),
            ("honor", "honour"),
            ("honors", "honours"),
            ("honored", "honoured"),
            ("honoring", "honouring"),
            ("humor", "humour"),
            ("humors", "humours"),
            ("humored", "humoured"),
            ("labor", "labour"),
            ("labors", "labours"),
            ("labored", "laboured"),
            ("laboring", "labouring"),
            ("neighbor", "neighbour"),
            ("neighbors", "neighbours"),
            ("neighbored", "neighboured"),
            ("rumor", "rumour"),
            ("rumors", "rumours"),
            ("rumored", "rumoured"),
            ("vapor", "vapour"),
            ("vapors", "vapours"),
            ("tumor", "tumour"),
            ("tumors", "tumours"),
            ("armor", "armour"),
            ("armors", "armours"),
            ("armored", "armoured"),
            ("harbor", "harbour"),
            ("harbors", "harbours"),
            ("savor", "savour"),
            ("savors", "savourings"),
            ("vigor", "vigour"),
            ("valor", "valour"),
            ("ardor", "ardour"),
            ("clamor", "clamour"),
            ("demeanor", "demeanour"),
            ("enamor", "enamour"),
            ("fervor", "fervour"),
            ("rancor", "rancour"),
            ("splendor", "splendour"),
            ("savior", "saviour"),
            ("odor", "odour"),
            ("odors", "odours"),
        ],
    );

    add_pairs(
        &mut map,
        &[
            ("center", "centre"),
            ("centers", "centres"),
            ("centered", "centred"),
            ("fiber", "fibre"),
            ("fibers", "fibres"),
            ("liter", "litre"),
            ("meters", "metres"),
            ("theater", "theatre"),
            ("theaters", "theatres"),
            ("specter", "spectre"),
            ("luster", "lustre"),
            ("meager", "meagre"),
        ],
    );

    add_pairs(
        &mut map,
        &[
            ("analyze", "analyse"),
            ("analyzes", "analyses"),
            ("analyzed", "analysed"),
            ("analyzing", "analysing"),
            ("organize", "organise"),
            ("organizes", "organises"),
            ("organized", "organised"),
            ("organizing", "organising"),
            ("realize", "realise"),
            ("realizes", "realises"),
            ("realized", "realised"),
            ("realizing", "realising"),
            ("recognize", "recognise"),
            ("recognizes", "recognises"),
            ("recognized", "recognised"),
            ("prioritize", "prioritise"),
            ("optimize", "optimise"),
            ("maximize", "maximise"),
            ("minimize", "minimise"),
            ("civilization", "civilisation"),
            ("organization", "organisation"),
        ],
    );

    add_pairs(
        &mut map,
        &[
            ("defense", "defence"),
            ("defenses", "defences"),
            ("offense", "offence"),
            ("offenses", "offences"),
            ("license", "licence"),
            ("licenses", "licences"),
        ],
    );

    add_pairs(
        &mut map,
        &[
            ("gray", "grey"),
            ("grays", "greys"),
            ("grayed", "greyed"),
            ("graying", "greying"),
            ("grayer", "greyer"),
        ],
    );

    add_pairs(
        &mut map,
        &[
            ("traveled", "travelled"),
            ("traveler", "traveller"),
            ("traveling", "travelling"),
            ("labeled", "labelled"),
            ("labeling", "labelling"),
            ("canceled", "cancelled"),
            ("canceling", "cancelling"),
            ("fueled", "fuelled"),
            ("fueling", "fuelling"),
            ("jeweler", "jeweller"),
            ("jewelry", "jewellery"),
        ],
    );

    add_pairs(
        &mut map,
        &[
            ("aluminum", "aluminium"),
            ("airplane", "aeroplane"),
            ("mustache", "moustache"),
            ("pajamas", "pyjamas"),
            ("donut", "doughnut"),
            ("disk", "disc"),
            ("aging", "ageing"),
            ("encyclopedia", "encyclopaedia"),
            ("maneuver", "manoeuvre"),
            ("mold", "mould"),
            ("skeptic", "sceptic"),
            ("smolder", "smoulder"),
            ("woolen", "woollen"),
            ("tidbit", "titbit"),
            ("story", "storey"),
            ("stories", "storeys"),
            ("fulfill", "fulfil"),
            ("enrollment", "enrolment"),
        ],
    );

    map
});

/// Helper to add multiple word pairs to the map.
fn add_pairs(
    map: &mut HashMap<&'static str, &'static str>,
    pairs: &[(&'static str, &'static str)],
) {
    for (us, uk) in pairs {
        map.insert(us, uk);
    }
}

/// Get the appropriate spelling dictionary based on the selected source.
pub fn get_dictionary(source: SpellingDictionary) -> &'static HashMap<&'static str, &'static str> {
    match source {
        SpellingDictionary::Dwyl => &DWYL_DICTIONARY,
        SpellingDictionary::Cspell => &CSPELL_DICTIONARY,
    }
}

/// Convert US English spelling to British English using the specified dictionary.
pub fn convert_us_to_british_with_dict(text: &str, source: SpellingDictionary) -> String {
    let dictionary = get_dictionary(source);
    convert_with_dictionary(text, dictionary)
}

/// Core conversion logic using any dictionary.
fn convert_with_dictionary(text: &str, dictionary: &HashMap<&'static str, &'static str>) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut converted_words: Vec<String> = Vec::new();

    for word in words {
        let (prefix, core, suffix) = extract_word_parts(word);
        let core_lower = core.to_lowercase();

        let converted = dictionary.get(core_lower.as_str());

        let result_word = if let Some(&british) = converted {
            preserve_case_pattern(core, british)
        } else if core_lower.ends_with("es") {
            let base = &core_lower[..core_lower.len() - 2];
            if let Some(&british_base) = dictionary.get(base) {
                let british_form = format!("{}es", british_base);
                preserve_case_pattern(core, &british_form)
            } else {
                core.to_string()
            }
        } else if core_lower.ends_with("s") && core_lower.len() > 1 {
            let base = &core_lower[..core_lower.len() - 1];
            if let Some(&british_base) = dictionary.get(base) {
                let british_form = if british_base.ends_with('s') {
                    british_base.to_string()
                } else {
                    format!("{}s", british_base)
                };
                preserve_case_pattern(core, &british_form)
            } else {
                core.to_string()
            }
        } else if core_lower.ends_with("ed") {
            let base = &core_lower[..core_lower.len() - 2];
            if let Some(&british_base) = dictionary.get(base) {
                let british_form = format!("{}ed", british_base);
                preserve_case_pattern(core, &british_form)
            } else if core_lower.ends_with("ied") {
                let base = format!("{}y", &core_lower[..core_lower.len() - 3]);
                if let Some(&british_base) = dictionary.get(base.as_str()) {
                    let british_form = format!(
                        "{}ied",
                        british_base.strip_suffix('e').unwrap_or(british_base)
                    );
                    preserve_case_pattern(core, &british_form)
                } else {
                    core.to_string()
                }
            } else {
                core.to_string()
            }
        } else if core_lower.ends_with("ing") {
            let base = &core_lower[..core_lower.len() - 3];
            if let Some(&british_base) = dictionary.get(base) {
                let british_form = format!("{}ing", british_base);
                preserve_case_pattern(core, &british_form)
            } else if core_lower.ends_with("ying") {
                let base = format!("{}y", &core_lower[..core_lower.len() - 4]);
                if let Some(&british_base) = dictionary.get(base.as_str()) {
                    let british_form = format!(
                        "{}ying",
                        british_base.strip_suffix('e').unwrap_or(british_base)
                    );
                    preserve_case_pattern(core, &british_form)
                } else {
                    core.to_string()
                }
            } else {
                core.to_string()
            }
        } else if core_lower.ends_with("er") {
            let base = &core_lower[..core_lower.len() - 2];
            if let Some(&british_base) = dictionary.get(base) {
                let british_form = format!("{}er", british_base);
                preserve_case_pattern(core, &british_form)
            } else {
                core.to_string()
            }
        } else if core_lower.ends_with("est") {
            let base = &core_lower[..core_lower.len() - 3];
            if let Some(&british_base) = dictionary.get(base) {
                let british_form = format!("{}est", british_base);
                preserve_case_pattern(core, &british_form)
            } else {
                core.to_string()
            }
        } else {
            core.to_string()
        };

        converted_words.push(format!("{}{}{}", prefix, result_word, suffix));
    }

    converted_words.join(" ")
}

/// Extracts leading punctuation, core word, and trailing punctuation from a word.
fn extract_word_parts(word: &str) -> (String, &str, String) {
    let chars: Vec<char> = word.chars().collect();

    let start = chars.iter().position(|c| c.is_alphanumeric()).unwrap_or(0);
    let end = chars.iter().rposition(|c| c.is_alphanumeric()).unwrap_or(0);

    if start > end || chars.iter().all(|c| !c.is_alphanumeric()) {
        return (word.to_string(), "", String::new());
    }

    let start_byte: usize = chars[..start].iter().map(|c| c.len_utf8()).sum();
    let core_len: usize = chars[start..=end].iter().map(|c| c.len_utf8()).sum();
    let end_byte = start_byte + core_len;

    let prefix: String = chars[..start].iter().collect();
    let core = &word[start_byte..end_byte];
    let suffix: String = chars[end + 1..].iter().collect();

    (prefix, core, suffix)
}

/// Preserves the case pattern from the original word.
fn preserve_case_pattern(original: &str, replacement: &str) -> String {
    if original.is_empty() || replacement.is_empty() {
        return replacement.to_string();
    }

    let original_chars: Vec<char> = original.chars().collect();
    let replacement_chars: Vec<char> = replacement.chars().collect();

    // All uppercase
    if original_chars.iter().all(|c| c.is_uppercase()) {
        return replacement.to_uppercase();
    }

    // Title case (first letter uppercase)
    if original_chars[0].is_uppercase() {
        let mut result = String::new();
        for (i, c) in replacement_chars.iter().enumerate() {
            if i == 0 {
                result.push(c.to_uppercase().next().unwrap());
            } else {
                result.push(*c);
            }
        }
        return result;
    }

    // Lowercase or mixed case - use replacement as-is
    replacement.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dwyl_basic_conversion() {
        let dict = SpellingDictionary::Dwyl;
        assert_eq!(convert_us_to_british_with_dict("color", dict), "colour");
        assert_eq!(convert_us_to_british_with_dict("colors", dict), "colours");
        assert_eq!(convert_us_to_british_with_dict("gray", dict), "grey");
    }

    #[test]
    fn test_cspell_basic_conversion() {
        let dict = SpellingDictionary::Cspell;
        assert_eq!(convert_us_to_british_with_dict("color", dict), "colour");
        assert_eq!(
            convert_us_to_british_with_dict("organize", dict),
            "organise"
        );
    }

    #[test]
    fn test_case_preservation() {
        let dict = SpellingDictionary::Dwyl;
        assert_eq!(convert_us_to_british_with_dict("Color", dict), "Colour");
        assert_eq!(convert_us_to_british_with_dict("COLOR", dict), "COLOUR");
        assert_eq!(convert_us_to_british_with_dict("color", dict), "colour");
    }

    #[test]
    fn test_punctuation() {
        let dict = SpellingDictionary::Dwyl;
        assert_eq!(convert_us_to_british_with_dict("color.", dict), "colour.");
        assert_eq!(
            convert_us_to_british_with_dict("\"color\"", dict),
            "\"colour\""
        );
    }

    #[test]
    fn test_plural_forms() {
        let dict = SpellingDictionary::Dwyl;
        assert_eq!(
            convert_us_to_british_with_dict("analyzed", dict),
            "analysed"
        );
        assert_eq!(
            convert_us_to_british_with_dict("analyzing", dict),
            "analysing"
        );
        assert_eq!(
            convert_us_to_british_with_dict("traveled", dict),
            "travelled"
        );
    }

    #[test]
    fn test_no_conversion_for_excluded_words() {
        let dict = SpellingDictionary::Dwyl;
        assert_eq!(convert_us_to_british_with_dict("check", dict), "check");
        assert_eq!(convert_us_to_british_with_dict("tire", dict), "tire");
    }

    #[test]
    fn test_sentence() {
        let dict = SpellingDictionary::Dwyl;
        let input = "The gray color of the center was organized.";
        let expected = "The grey colour of the centre was organised.";
        assert_eq!(convert_us_to_british_with_dict(input, dict), expected);
    }

    #[test]
    fn test_dictionary_size() {
        let dwyl = get_dictionary(SpellingDictionary::Dwyl);
        assert!(
            dwyl.len() > 100,
            "DWYL dictionary should have at least 100 entries"
        );

        let cspell = get_dictionary(SpellingDictionary::Cspell);
        assert!(
            cspell.len() > 50,
            "CSpell dictionary should have at least 50 entries"
        );
    }
}