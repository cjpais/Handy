use std::{collections::HashSet, fmt::Display};

use log::{error, trace, warn};
use nom::{
    IResult, Parser,
    branch::permutation,
    bytes::complete::{tag, take_until},
    character::{
        complete::{char, multispace0},
        streaming::u32,
    },
    combinator::opt,
    error::ParseError,
    multi::many0,
    sequence::{delimited, pair, preceded, separated_pair, terminated},
};

use crate::InputError;

type Keycode = u32;

pub trait Parse {
    fn parse(input: &str) -> IResult<&str, Self>
    where
        Self: Sized;
}

#[derive(Debug, PartialEq, PartialOrd, Ord, Eq, Clone)]
pub struct ParsedKeymap {
    keycodes: Keycodes,
    // Don't parse this, just keep it as is
    types: Option<String>,
    // Don't parse this, just keep it as is
    compatibility: Option<String>,
    symbols: Symbols,
    // Don't parse this, just keep it as is
    geometry: Option<String>,
}

impl ParsedKeymap {
    // TODO: Add tests for this function
    /// Try to find an unused keycode and identifier to map the provided keyname
    /// to. Returns the keycode the key is now mapped to
    ///
    /// # Errors
    /// The function fails if no unused keycode can be found or if all
    /// identifiers from <0000> to <9999> are already used
    pub fn map_key(&mut self, key_name: &str, is_wayland: bool) -> crate::InputResult<u16> {
        // Even if the mimimum is 8, never use 8. This is because 8 is special. 8-8=0
        // and the value 0 stands for "NoSymbol". Some clients disregard the keymap and
        // always interpret keycode 8 as NoSymbol.
        let minimum = self.keycodes.minimum.max(9);
        // Maximum on X11 is 255, but on Wayland we can use keycodes up to u16::MAX
        let maximum = if is_wayland { u16::MAX as u32 } else { 255 };

        // Find an unused keycode
        let free_keycode_u32 = (minimum..maximum)
            .find(|raw| {
                !self
                    .keycodes
                    .keycode_mappings
                    .iter()
                    .any(|entry| *raw == entry.code)
            })
            .ok_or_else(|| InputError::Mapping("no available keycode".to_string()))?;
        let free_keycode_u16 = u16::try_from(free_keycode_u32)
            .map_err(|_| InputError::Mapping("the available keycode exceeds u16::MAX".to_string()));

        // Find an unused identifier
        let free_identifier = (0..=9999)
            .rev()
            .map(|idx| format!("{idx:0>4}"))
            .filter(|potential_identifier_name| {
                !self
                    .keycodes
                    .keycode_mappings
                    .iter()
                    .any(|entry| *potential_identifier_name == entry.identifier.identifier)
            })
            .find(|potential_identifier_name| {
                !self
                    .symbols
                    .keys
                    .iter()
                    .any(|(identifier, _)| *potential_identifier_name == identifier.identifier)
            })
            .ok_or_else(|| InputError::Mapping("no available identifier".to_string()))?;
        let free_identifier = Identifier {
            identifier: free_identifier,
        };

        // Add free identifier and keycode to keymap
        self.keycodes.keycode_mappings.push(KeycodeEntry {
            identifier: free_identifier.clone(),
            code: free_keycode_u32,
        });

        let symbols_string = format!("{{\t[ {key_name}, {key_name} ] }}");
        self.symbols.keys.push((free_identifier, symbols_string));

        // Update the maximum if it is needed
        self.keycodes.maximum = self.keycodes.maximum.max(free_keycode_u32);
        free_keycode_u16
    }

    pub fn copy_maps_for_keycodes(
        &mut self,
        other_keymap: &Self,
        keycodes_to_copy: &HashSet<Keycode>,
    ) {
        let keycode_entrys: Vec<_> = other_keymap
            .keycodes
            .keycode_mappings
            .iter()
            .filter(
                |KeycodeEntry {
                     identifier: _,
                     code,
                 }| keycodes_to_copy.contains(code),
            )
            .cloned()
            .collect();

        let max_keycode = other_keymap
            .keycodes
            .keycode_mappings
            .iter()
            .max_by_key(|x| x.code);
        if let Some(max_keycode) = max_keycode {
            self.keycodes.maximum = self.keycodes.maximum.max(max_keycode.code);
        }
        // We can ignore aliases here because they are only used by the original keymap
        let identifiers: Vec<_> = keycode_entrys
            .iter()
            .map(
                |KeycodeEntry {
                     identifier,
                     code: _,
                 }| identifier,
            )
            .collect();

        let key_entrys: Vec<_> = other_keymap
            .symbols
            .keys
            .iter()
            .filter(|(identifier, _)| identifiers.contains(&identifier))
            .collect();

        // Add the found entries to the keymap, but only if they are not in it already
        for keycode_entry in keycode_entrys {
            if !self.keycodes.keycode_mappings.contains(&keycode_entry) {
                let symbols_entry = key_entrys
                    .iter()
                    .find(|(i, _)| *i == keycode_entry.identifier);
                if let Some(symbols_entry) = symbols_entry {
                    self.symbols.keys.push((*symbols_entry).clone());
                }
                self.keycodes.keycode_mappings.push(keycode_entry);
            }
        }
    }
}

impl TryFrom<&str> for ParsedKeymap {
    type Error = ();

    fn try_from(keymap_str: &str) -> Result<Self, Self::Error> {
        trace!("ParsedKeymap::try_from({keymap_str})");
        let (remaining, parsed_keymap) = ParsedKeymap::parse(keymap_str).map_err(|_| {
            error!("parsing keymap failed");
        })?;
        if !remaining.is_empty() && remaining != "\0" {
            warn!("not all of the keymap could be parsed. Remaining:\n\"{remaining}\"");
        }
        Ok(parsed_keymap)
    }
}

impl Display for ParsedKeymap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "xkb_keymap {{")?;
        writeln!(f, "{}", self.keycodes)?;
        if let Some(types) = &self.types {
            writeln!(f, "xkb_types {types}\n}};\n",)?;
        }
        if let Some(compatibility) = &self.compatibility {
            writeln!(f, "xkb_compatibility {compatibility}\n}};\n")?;
        }
        writeln!(f, "{}", self.symbols)?;

        if let Some(geometry) = &self.geometry {
            writeln!(f, "xkb_geometry {geometry}\n}};\n")?;
        }
        writeln!(f, "}};")
    }
}

impl Parse for ParsedKeymap {
    fn parse(input: &str) -> IResult<&str, Self> {
        let types_parser = delimited(ws(tag("xkb_types")), take_until("\n};\n"), tag("\n};\n"))
            .map(|s: &str| s.to_string());
        let compatibility_parser = delimited(
            ws(tag("xkb_compatibility")),
            take_until("\n};\n"),
            tag("\n};\n"),
        )
        .map(|s: &str| s.to_string());
        let geometry_parser =
            delimited(ws(tag("xkb_geometry")), take_until("\n};\n"), tag("\n};\n"))
                .map(|s: &str| s.to_string());
        let content_parser = permutation((
            Keycodes::parse,
            opt(types_parser),
            opt(compatibility_parser),
            Symbols::parse,
            opt(geometry_parser),
        ));
        let mut keymap_parser = preceded(pair(ws(tag("xkb_keymap")), tag("{\n")), content_parser);

        let (remaining, (keycodes, types, compatibility, symbols, geometry)) =
            keymap_parser.parse(input)?;
        let (remaining, _) = preceded(multispace0, tag("};\n")).parse(remaining)?;

        Ok((
            remaining,
            Self {
                keycodes,
                types,
                compatibility,
                symbols,
                geometry,
            },
        ))
    }
}

/// The keycodes component of a keyboard mapping specifies the range and
/// interpretation of the raw keycodes reported by the device. It sets the
/// keycodes symbolic name, the minimum and maximum legal codes for the
/// keyboard, and the symbolic name for each key. The keycodes component might
/// also contain aliases for some keys, symbolic names for some indicators and a
/// description of which indicators are physically present.
#[derive(Debug, PartialEq, PartialOrd, Ord, Eq, Clone)]
struct Keycodes {
    name: Name,
    minimum: Keycode,
    maximum: Keycode,
    keycode_mappings: Vec<KeycodeEntry>,
    max_len_identifier: usize, // Max length of all identifiers
    indicators: Vec<IndicatorEntry>,
    aliases: Vec<AliasEntry>,
}

impl Display for Keycodes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "xkb_keycodes {} {{", self.name)?;
        writeln!(f, "    minimum = {};", self.minimum)?;
        writeln!(f, "    maximum = {};", self.maximum)?;
        for keycode_mapping in &self.keycode_mappings {
            for _ in keycode_mapping.identifier.identifier.len()..self.max_len_identifier {
                write!(f, " ")?;
            }
            writeln!(f, "    {keycode_mapping}")?;
        }
        for indicators in &self.indicators {
            writeln!(f, "    {indicators}")?;
        }
        for alias in &self.aliases {
            writeln!(f, "    {alias}")?;
        }
        writeln!(f, "}};")
    }
}

impl Parse for Keycodes {
    fn parse(input: &str) -> IResult<&str, Self> {
        let (remaining, name) = parse_section(input, "xkb_keycodes").unwrap();
        let minimum_parser = delimited(pair(ws(tag("minimum")), ws(tag("="))), u32, tag(";"));
        let maximum_parser = delimited(pair(ws(tag("maximum")), ws(tag("="))), u32, tag(";"));
        let content_parser = permutation((
            minimum_parser,
            maximum_parser,
            many0(KeycodeEntry::parse),
            many0(IndicatorEntry::parse),
            many0(AliasEntry::parse),
        ));
        let (remaining, (minimum, maximum, keycodes, indicators, aliases)) =
            terminated(content_parser, ws(tag("};"))).parse(remaining)?;

        let mut max_len_identifier = 0;
        for KeycodeEntry { identifier, .. } in &keycodes {
            max_len_identifier = max_len_identifier.max(identifier.identifier.len());
        }
        Ok((
            remaining,
            Keycodes {
                name,
                minimum,
                maximum,
                keycode_mappings: keycodes,
                max_len_identifier,
                indicators,
                aliases,
            },
        ))
    }
}

#[derive(Debug, PartialEq, PartialOrd, Ord, Eq, Clone)]
struct KeycodeEntry {
    identifier: Identifier,
    code: Keycode,
}

impl Display for KeycodeEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} = {};", self.identifier, self.code)
    }
}

impl Parse for KeycodeEntry {
    fn parse(input: &str) -> IResult<&str, Self> {
        let mapping_parser = separated_pair(Identifier::parse, ws(char('=')), u32);
        terminated(ws(mapping_parser), tag(";"))
            .parse(input)
            .map(|(r, (identifier, code))| (r, Self { identifier, code }))
    }
}

#[derive(Debug, PartialEq, PartialOrd, Ord, Eq, Clone)]
struct IndicatorEntry {
    idx: u32,
    name: Name,
}

impl Display for IndicatorEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "indicator {} = {};", self.idx, self.name)
    }
}

impl Parse for IndicatorEntry {
    fn parse(input: &str) -> IResult<&str, Self> {
        let mapping_parser = separated_pair(u32, ws(char('=')), Name::parse);
        delimited(ws(tag("indicator")), mapping_parser, tag(";"))
            .parse(input)
            .map(|(r, (idx, name))| (r, Self { idx, name }))
    }
}

#[derive(Debug, PartialEq, PartialOrd, Ord, Eq, Clone)]
struct AliasEntry {
    alias: Identifier,
    name: Identifier,
}

impl Display for AliasEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "alias ")?;

        for _ in self.alias.identifier.len()..4 {
            write!(f, " ")?;
        }
        write!(f, "{} = ", self.alias)?;

        for _ in self.name.identifier.len()..4 {
            write!(f, " ")?;
        }
        write!(f, "{};", self.name)
    }
}

impl Parse for AliasEntry {
    fn parse(input: &str) -> IResult<&str, Self> {
        let mapping_parser = separated_pair(Identifier::parse, ws(char('=')), Identifier::parse);
        delimited(ws(tag("alias")), mapping_parser, tag(";"))
            .parse(input)
            .map(|(r, (a, n))| (r, Self { alias: a, name: n }))
    }
}

#[derive(Debug, PartialEq, PartialOrd, Ord, Eq, Clone)]
struct Name {
    name: String,
}

impl Display for Name {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "\"{}\"", self.name)
    }
}

impl Parse for Name {
    fn parse(input: &str) -> IResult<&str, Self> {
        let (remaining, name) = delimited(char('"'), take_until("\""), char('"')).parse(input)?;
        let name = Self {
            name: name.to_string(),
        };
        Ok((remaining, name))
    }
}

#[derive(Debug, PartialEq, PartialOrd, Ord, Eq, Clone)]
struct Identifier {
    identifier: String,
}

impl Display for Identifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<{}>", self.identifier)
    }
}

impl Parse for Identifier {
    fn parse(input: &str) -> IResult<&str, Self> {
        let (remaining, id) = delimited(char('<'), take_until(">"), char('>')).parse(input)?;
        let id = Self {
            identifier: id.to_string(),
        };
        Ok((remaining, id))
    }
}

#[derive(Debug, PartialEq, PartialOrd, Ord, Eq, Clone)]
struct Symbols {
    name: Name,
    groups: Vec<Name>,
    keys: Vec<(Identifier, String)>,
    max_len_identifier: usize, // Max length of all identifiers
    modifier_map: Vec<String>,
}

impl Display for Symbols {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "xkb_symbols {} {{", self.name)?;
        writeln!(f)?;
        for (idx, group) in self.groups.iter().enumerate() {
            writeln!(f, "    name[group{}]={group};", idx + 1)?;
        }
        writeln!(f)?;
        for (key_id, key_def) in &self.keys {
            write!(f, "    key ")?;
            // Leftpad
            for _ in key_id.identifier.len()..self.max_len_identifier {
                write!(f, " ")?;
            }
            writeln!(f, "{key_id} {key_def};")?;
        }
        for mod_map in &self.modifier_map {
            writeln!(f, "    modifier_map {mod_map};")?;
        }
        writeln!(f, "}};")
    }
}

impl Parse for Symbols {
    fn parse(input: &str) -> IResult<&str, Self> {
        let (remaining, name) = parse_section(input, "xkb_symbols").unwrap();
        let groups_parser = delimited(
            pair(ws(tag("name")), take_until("\"")),
            Name::parse,
            ws(tag(";")),
        );
        let key_parser = delimited(
            ws(tag("key ")),
            pair(ws(Identifier::parse), take_until(";")),
            ws(tag(";")),
        )
        .map(|(id, s)| (id, s.to_string()));
        let modifier_map_parser =
            delimited(ws(tag("modifier_map ")), take_until(";"), ws(tag(";")))
                .map(std::string::ToString::to_string);
        let content_parser = permutation((
            many0(groups_parser),
            many0(key_parser),
            many0(modifier_map_parser),
        ));

        let (remaining, (groups, keys, modifier_map)) =
            terminated(content_parser, ws(tag("};"))).parse(remaining)?;

        let mut max_len_identifier = 0;
        for (key_id, _) in &keys {
            max_len_identifier = max_len_identifier.max(key_id.identifier.len());
        }
        Ok((
            remaining,
            Symbols {
                name,
                groups,
                keys,
                max_len_identifier,
                modifier_map,
            },
        ))
    }
}

fn parse_section<'a>(input: &'a str, struct_tag: &str) -> IResult<&'a str, Name> {
    delimited(ws(tag(struct_tag)), Name::parse, ws(tag("{"))).parse(input)
}

/// A combinator that takes a parser `inner` and produces a parser that also
/// consumes both leading and trailing whitespace, returning the output of
/// `inner`.
pub fn ws<'a, O, E: ParseError<&'a str>, F>(inner: F) -> impl Parser<&'a str, Output = O, Error = E>
where
    F: Parser<&'a str, Output = O, Error = E>,
{
    delimited(multispace0, inner, multispace0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_section() {
        let parse_str = "xkb_keycodes \"(unnamed)\" {
    minimum = 8;";

        let correct_res = Ok((
            "minimum = 8;",
            Name {
                name: "(unnamed)".to_string(),
            },
        ));

        assert_eq!(parse_section(parse_str, "xkb_keycodes"), correct_res);
    }

    #[test]
    fn test_parse_keycodes() {
        let keycodes_str = "xkb_keycodes \"(unnamed)\" {
    minimum = 8;
    maximum = 255;
     <ESC> = 9;
        <> = 10;
      <UP> = 111;
    <VOL-> = 122;
    <VOL+> = 123;
     <CUT> = 145;
    <FK24> = 202;
    <LVL5> = 203;
     <ALT> = 204;
    <META> = 205;
    <I254> = 254;
    <I255> = 255;
    indicator 1 = \"Caps Lock\";
    indicator 2 = \"Num Lock\";
    indicator 13 = \"Group 2\";
    indicator 14 = \"Mouse Keys\";
    alias <AC12> = <BKSL>;
    alias <ALGR> = <RALT>;
    alias <MENU> = <COMP>;
    alias <HZTG> = <TLDE>;
};
";

        let correct_keycodes = vec![
            KeycodeEntry {
                identifier: Identifier {
                    identifier: "ESC".to_string(),
                },
                code: 9,
            },
            KeycodeEntry {
                identifier: Identifier {
                    identifier: "".to_string(),
                },
                code: 10,
            },
            KeycodeEntry {
                identifier: Identifier {
                    identifier: "UP".to_string(),
                },
                code: 111,
            },
            KeycodeEntry {
                identifier: Identifier {
                    identifier: "VOL-".to_string(),
                },
                code: 122,
            },
            KeycodeEntry {
                identifier: Identifier {
                    identifier: "VOL+".to_string(),
                },
                code: 123,
            },
            KeycodeEntry {
                identifier: Identifier {
                    identifier: "CUT".to_string(),
                },
                code: 145,
            },
            KeycodeEntry {
                identifier: Identifier {
                    identifier: "FK24".to_string(),
                },
                code: 202,
            },
            KeycodeEntry {
                identifier: Identifier {
                    identifier: "LVL5".to_string(),
                },
                code: 203,
            },
            KeycodeEntry {
                identifier: Identifier {
                    identifier: "ALT".to_string(),
                },
                code: 204,
            },
            KeycodeEntry {
                identifier: Identifier {
                    identifier: "META".to_string(),
                },
                code: 205,
            },
            KeycodeEntry {
                identifier: Identifier {
                    identifier: "I254".to_string(),
                },
                code: 254,
            },
            KeycodeEntry {
                identifier: Identifier {
                    identifier: "I255".to_string(),
                },
                code: 255,
            },
        ];

        let correct_indicators = vec![
            IndicatorEntry {
                idx: 1,
                name: Name {
                    name: "Caps Lock".to_string(),
                },
            },
            IndicatorEntry {
                idx: 2,
                name: Name {
                    name: "Num Lock".to_string(),
                },
            },
            IndicatorEntry {
                idx: 13,
                name: Name {
                    name: "Group 2".to_string(),
                },
            },
            IndicatorEntry {
                idx: 14,
                name: Name {
                    name: "Mouse Keys".to_string(),
                },
            },
        ];

        let correct_aliases = vec![
            AliasEntry {
                alias: Identifier {
                    identifier: "AC12".to_string(),
                },
                name: Identifier {
                    identifier: "BKSL".to_string(),
                },
            },
            AliasEntry {
                alias: Identifier {
                    identifier: "ALGR".to_string(),
                },
                name: Identifier {
                    identifier: "RALT".to_string(),
                },
            },
            AliasEntry {
                alias: Identifier {
                    identifier: "MENU".to_string(),
                },
                name: Identifier {
                    identifier: "COMP".to_string(),
                },
            },
            AliasEntry {
                alias: Identifier {
                    identifier: "HZTG".to_string(),
                },
                name: Identifier {
                    identifier: "TLDE".to_string(),
                },
            },
        ];

        let correct_keycodes_struct = Keycodes {
            name: Name {
                name: "(unnamed)".to_string(),
            },
            minimum: 8,
            maximum: 255,
            keycode_mappings: correct_keycodes,
            max_len_identifier: 4,
            indicators: correct_indicators,
            aliases: correct_aliases,
        };

        println!("{correct_keycodes_struct}");

        assert_eq!(
            Keycodes::parse(keycodes_str),
            Ok(("", correct_keycodes_struct))
        );
    }

    #[test]
    fn test_parse_keycode_entry() {
        let test_cases = vec![
            (
                "<ESC> = 9;",
                Ok((
                    "",
                    KeycodeEntry {
                        identifier: Identifier {
                            identifier: "ESC".to_string(),
                        },
                        code: 9,
                    },
                )),
            ),
            (
                "<AE01> = 10;",
                Ok((
                    "",
                    KeycodeEntry {
                        identifier: Identifier {
                            identifier: "AE01".to_string(),
                        },
                        code: 10,
                    },
                )),
            ),
            (
                "<TAB> = 23;",
                Ok((
                    "",
                    KeycodeEntry {
                        identifier: Identifier {
                            identifier: "TAB".to_string(),
                        },
                        code: 23,
                    },
                )),
            ),
            (
                "<UP> = 111;",
                Ok((
                    "",
                    KeycodeEntry {
                        identifier: Identifier {
                            identifier: "UP".to_string(),
                        },
                        code: 111,
                    },
                )),
            ),
            (
                "<VOL-> = 122;",
                Ok((
                    "",
                    KeycodeEntry {
                        identifier: Identifier {
                            identifier: "VOL-".to_string(),
                        },
                        code: 122,
                    },
                )),
            ),
            (
                "<VOL+> = 123;",
                Ok((
                    "",
                    KeycodeEntry {
                        identifier: Identifier {
                            identifier: "VOL+".to_string(),
                        },
                        code: 123,
                    },
                )),
            ),
            (
                "<> = 0;",
                Ok((
                    "",
                    KeycodeEntry {
                        identifier: Identifier {
                            identifier: "".to_string(),
                        },
                        code: 0,
                    },
                )),
            ),
            (
                "<I255> = 255;",
                Ok((
                    "",
                    KeycodeEntry {
                        identifier: Identifier {
                            identifier: "I255".to_string(),
                        },
                        code: 255,
                    },
                )),
            ),
        ];
        for (parse_str, correct_res) in &test_cases {
            assert_eq!(KeycodeEntry::parse(parse_str), *correct_res);
        }
    }

    #[test]
    fn test_parse_indicator() {
        let test_cases = vec![
            (
                "indicator 1 = \"Caps Lock\";",
                Ok((
                    "",
                    IndicatorEntry {
                        idx: 1,
                        name: Name {
                            name: "Caps Lock".to_string(),
                        },
                    },
                )),
            ),
            (
                "indicator 1 = \"Caps Lock\";
    indicator 2 = \"Num Lock\";
    indicator 3 = \"Scroll Lock\";",
                Ok((
                    "
    indicator 2 = \"Num Lock\";
    indicator 3 = \"Scroll Lock\";",
                    IndicatorEntry {
                        idx: 1,
                        name: Name {
                            name: "Caps Lock".to_string(),
                        },
                    },
                )),
            ),
            (
                "indicator 2 = \"Num Lock\";",
                Ok((
                    "",
                    IndicatorEntry {
                        idx: 2,
                        name: Name {
                            name: "Num Lock".to_string(),
                        },
                    },
                )),
            ),
            (
                "indicator 3 = \"Scroll Lock\";",
                Ok((
                    "",
                    IndicatorEntry {
                        idx: 3,
                        name: Name {
                            name: "Scroll Lock".to_string(),
                        },
                    },
                )),
            ),
            (
                "indicator 4 = \"Compose\";",
                Ok((
                    "",
                    IndicatorEntry {
                        idx: 4,
                        name: Name {
                            name: "Compose".to_string(),
                        },
                    },
                )),
            ),
            (
                "indicator 5 = \"Kana\";",
                Ok((
                    "",
                    IndicatorEntry {
                        idx: 5,
                        name: Name {
                            name: "Kana".to_string(),
                        },
                    },
                )),
            ),
            (
                "indicator 6 = \"Sleep\";",
                Ok((
                    "",
                    IndicatorEntry {
                        idx: 6,
                        name: Name {
                            name: "Sleep".to_string(),
                        },
                    },
                )),
            ),
            (
                "indicator 7 = \"Suspend\";",
                Ok((
                    "",
                    IndicatorEntry {
                        idx: 7,
                        name: Name {
                            name: "Suspend".to_string(),
                        },
                    },
                )),
            ),
            (
                "indicator 8 = \"Mute\";",
                Ok((
                    "",
                    IndicatorEntry {
                        idx: 8,
                        name: Name {
                            name: "Mute".to_string(),
                        },
                    },
                )),
            ),
            (
                "indicator 9 = \"Misc\";",
                Ok((
                    "",
                    IndicatorEntry {
                        idx: 9,
                        name: Name {
                            name: "Misc".to_string(),
                        },
                    },
                )),
            ),
            (
                "indicator 10 = \"Mail\";",
                Ok((
                    "",
                    IndicatorEntry {
                        idx: 10,
                        name: Name {
                            name: "Mail".to_string(),
                        },
                    },
                )),
            ),
            (
                "indicator 11 = \"Charging\";",
                Ok((
                    "",
                    IndicatorEntry {
                        idx: 11,
                        name: Name {
                            name: "Charging".to_string(),
                        },
                    },
                )),
            ),
            (
                "indicator 12 = \"Shift Lock\";",
                Ok((
                    "",
                    IndicatorEntry {
                        idx: 12,
                        name: Name {
                            name: "Shift Lock".to_string(),
                        },
                    },
                )),
            ),
            (
                "indicator 13 = \"Group 2\";",
                Ok((
                    "",
                    IndicatorEntry {
                        idx: 13,
                        name: Name {
                            name: "Group 2".to_string(),
                        },
                    },
                )),
            ),
            (
                "indicator 14 = \"Mouse Keys\";",
                Ok((
                    "",
                    IndicatorEntry {
                        idx: 14,
                        name: Name {
                            name: "Mouse Keys".to_string(),
                        },
                    },
                )),
            ),
        ];
        for (parse_str, correct_res) in &test_cases {
            assert_eq!(IndicatorEntry::parse(parse_str), *correct_res);
        }
    }

    #[test]
    fn test_parse_alias() {
        let test_cases = vec![
            (
                "alias <I141> = <COPY>;",
                Ok((
                    "",
                    AliasEntry {
                        alias: Identifier {
                            identifier: "I141".to_string(),
                        },
                        name: Identifier {
                            identifier: "COPY".to_string(),
                        },
                    },
                )),
            ),
            (
                "\n    alias <I141> = <COPY>;\n    ",
                Ok((
                    "\n    ",
                    AliasEntry {
                        alias: Identifier {
                            identifier: "I141".to_string(),
                        },
                        name: Identifier {
                            identifier: "COPY".to_string(),
                        },
                    },
                )),
            ),
            (
                "alias <I123> = <VOL+>;",
                Ok((
                    "",
                    AliasEntry {
                        alias: Identifier {
                            identifier: "I123".to_string(),
                        },
                        name: Identifier {
                            identifier: "VOL+".to_string(),
                        },
                    },
                )),
            ),
        ];
        for (parse_str, correct_res) in &test_cases {
            assert_eq!(AliasEntry::parse(parse_str), *correct_res);
        }
    }

    #[test]
    fn test_parse_identifier() {
        let test_cases = vec![
            (
                "<ESC>",
                Ok((
                    "",
                    Identifier {
                        identifier: "ESC".to_string(),
                    },
                )),
            ),
            (
                "<I255>",
                Ok((
                    "",
                    Identifier {
                        identifier: "I255".to_string(),
                    },
                )),
            ),
            (
                "<TAB>",
                Ok((
                    "",
                    Identifier {
                        identifier: "TAB".to_string(),
                    },
                )),
            ),
            (
                "<I255>",
                Ok((
                    "",
                    Identifier {
                        identifier: "I255".to_string(),
                    },
                )),
            ),
            (
                "<UP>",
                Ok((
                    "",
                    Identifier {
                        identifier: "UP".to_string(),
                    },
                )),
            ),
            (
                "<VOL->",
                Ok((
                    "",
                    Identifier {
                        identifier: "VOL-".to_string(),
                    },
                )),
            ),
            (
                "<VOL+>",
                Ok((
                    "",
                    Identifier {
                        identifier: "VOL+".to_string(),
                    },
                )),
            ),
            (
                "<I167>",
                Ok((
                    "",
                    Identifier {
                        identifier: "I167".to_string(),
                    },
                )),
            ),
            (
                "<>",
                Ok((
                    "",
                    Identifier {
                        identifier: "".to_string(),
                    },
                )),
            ),
            (
                "<LatM>",
                Ok((
                    "",
                    Identifier {
                        identifier: "LatM".to_string(),
                    },
                )),
            ),
        ];
        for (id_str, correct_id) in &test_cases {
            assert_eq!(Identifier::parse(id_str), *correct_id);
        }
    }

    #[test]
    fn test_parse_name() {
        let test_cases = vec![
            (
                "\"(unnamed)\"",
                Ok((
                    "",
                    Name {
                        name: "(unnamed)".to_string(),
                    },
                )),
            ),
            (
                "\"enigo\"",
                Ok((
                    "",
                    Name {
                        name: "enigo".to_string(),
                    },
                )),
            ),
        ];
        for (name_str, correct_name) in &test_cases {
            assert_eq!(Name::parse(name_str), *correct_name);
        }
    }

    #[test]
    fn test_parse_symbols() {
        let symbols_str = r#"
xkb_symbols "(unnamed)" {

    name[group1]="German";
    name[group2]="English (UK)";

    key     <> {         [           U8A9E ] };
    key  <ESC> {         [          Escape ] };
    key <AC11> {
        type[group1]= "FOUR_LEVEL_SEMIALPHABETIC",
        type[group2]= "FOUR_LEVEL",
        symbols[Group1]= [      adiaeresis,      Adiaeresis, dead_circumflex,      dead_caron ],
        symbols[Group2]= [      apostrophe,              at, dead_circumflex,      dead_caron ]
    };
    key <KPSU> {
        type= "CTRL+ALT",
        symbols[Group1]= [     KP_Subtract,     KP_Subtract,     KP_Subtract,     KP_Subtract,  XF86Prev_VMode ]
    };
    key <FK23> {
        type= "PC_SHIFT_SUPER_LEVEL2",
        symbols[Group1]= [ XF86TouchpadOff,   XF86Assistant ]
    };
    key <LVL5> {         [ ISO_Level5_Shift ] };
    key  <ALT> {         [        NoSymbol,           Alt_L ] };
    key <I208> {         [   XF86AudioPlay ] };
    key <I209> {         [  XF86AudioPause ] };
    modifier_map Control { <LCTL> };
    modifier_map Shift { <LFSH> };
    modifier_map Shift { <RTSH> };
    modifier_map Mod1 { <LALT> };
    modifier_map Lock { <CAPS> };
    modifier_map Control { <RCTL> };
    modifier_map Mod4 { <LWIN> };
    modifier_map Mod4 { <RWIN> };
};"#;

        let correct_symbols = Symbols {
            name: Name {
                name: "(unnamed)".to_string(),
            },
            groups: vec![
                Name {
                    name: "German".to_string(),
                },
                Name {
                    name: "English (UK)".to_string(),
                },
            ],
            keys: vec![(Identifier{ identifier: "".to_string() },"{         [           U8A9E ] }".to_string()),
    (Identifier{ identifier: "ESC".to_string() },"{         [          Escape ] }".to_string()),
    (Identifier{ identifier: "AC11".to_string() },"{
        type[group1]= \"FOUR_LEVEL_SEMIALPHABETIC\",
        type[group2]= \"FOUR_LEVEL\",
        symbols[Group1]= [      adiaeresis,      Adiaeresis, dead_circumflex,      dead_caron ],
        symbols[Group2]= [      apostrophe,              at, dead_circumflex,      dead_caron ]
    }".to_string()),
    (Identifier{ identifier: "KPSU".to_string() },"{
        type= \"CTRL+ALT\",
        symbols[Group1]= [     KP_Subtract,     KP_Subtract,     KP_Subtract,     KP_Subtract,  XF86Prev_VMode ]
    }".to_string()),
    (Identifier{ identifier: "FK23".to_string() },"{
        type= \"PC_SHIFT_SUPER_LEVEL2\",
        symbols[Group1]= [ XF86TouchpadOff,   XF86Assistant ]
    }".to_string()),
    (Identifier{ identifier: "LVL5".to_string() },"{         [ ISO_Level5_Shift ] }".to_string()),
    (Identifier{ identifier: "ALT".to_string() },"{         [        NoSymbol,           Alt_L ] }".to_string()),
    (Identifier{ identifier: "I208".to_string() },"{         [   XF86AudioPlay ] }".to_string()),
    (Identifier{ identifier: "I209".to_string() },"{         [  XF86AudioPause ] }".to_string()),



            ],
            max_len_identifier:4,
            modifier_map: vec![r#"Control { <LCTL> }"#.to_string(),
    r#"Shift { <LFSH> }"#.to_string(),
    r#"Shift { <RTSH> }"#.to_string(),
    r#"Mod1 { <LALT> }"#.to_string(),
    r#"Lock { <CAPS> }"#.to_string(),
    r#"Control { <RCTL> }"#.to_string(),
    r#"Mod4 { <LWIN> }"#.to_string(),
    r#"Mod4 { <RWIN> }"#.to_string()],
        };

        println!("{correct_symbols}");

        assert_eq!(Symbols::parse(symbols_str), Ok(("", correct_symbols)));
    }
}
