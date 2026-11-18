//! Which application protocol a STEP file declares in its `FILE_SCHEMA`: AP203, AP214, or AP242 and
//! its edition. A file is read the same whatever it declares; an AP242 edition that isn't given or
//! isn't known, and a schema that isn't one of these, read with a warning, never a refusal.
//!
//! Every AP242 edition has the same schema name; only the object identifier after it, in braces,
//! tells the editions apart. Written from public descriptions of the schema names and identifiers
//! (STEP Tools' notes on each protocol, MBx-IF's table of EXPRESS schemas). They differ on Edition
//! 4's identifier (`442 5 1 4` and `442 7 1 4`), so both are read as Edition 4.
//!
//! Input is hostile: at most [`MAX_SCHEMAS`] names are looked at, at most `MAX_ARCS` arcs of one
//! identifier are read, and a warning repeats at most `MAX_SHOWN` characters of the file's text.
//!
//! Origin: new in solidcore, with no SolidCraft or SMLib counterpart.
//!
//! Deviations: none.

use std::fmt;

/// Most `FILE_SCHEMA` names looked at for a protocol.
pub const MAX_SCHEMAS: usize = 16;
/// Most arcs read from one object identifier: the AP242 identifiers have seven.
const MAX_ARCS: usize = 16;
/// Most characters of the file's own text one warning repeats.
const MAX_SHOWN: usize = 80;

/// The AP242 schema name, shared by every edition.
const AP242: &str = "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF";
/// AP203's names: Edition 1, Edition 2, and the interim name met in older files.
const AP203: [&str; 3] = [
    "CONFIG_CONTROL_DESIGN",
    "AP203_CONFIGURATION_CONTROLLED_3D_DESIGN_OF_MECHANICAL_PARTS_AND_ASSEMBLIES_MIM_LF",
    "CONFIGURATION_CONTROL_3D_DESIGN_MIM_LF",
];
/// AP214's names: every edition's, and the older conformance-class name.
const AP214: [&str; 2] = ["AUTOMOTIVE_DESIGN", "AUTOMOTIVE_DESIGN_CC2"];

/// An edition of AP242.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ap242Edition {
    /// Edition 1: `{ 1 0 10303 442 1 1 4 }`.
    First,
    /// Edition 2: `{ 1 0 10303 442 3 1 4 }`.
    Second,
    /// Edition 3: `{ 1 0 10303 442 4 1 4 }`.
    Third,
    /// Edition 4: `{ 1 0 10303 442 7 1 4 }`, or `{ 1 0 10303 442 5 1 4 }` as some sources give it.
    Fourth,
}

impl Ap242Edition {
    /// The edition's number, 1 to 4.
    pub fn number(self) -> u8 {
        match self {
            Ap242Edition::First => 1,
            Ap242Edition::Second => 2,
            Ap242Edition::Third => 3,
            Ap242Edition::Fourth => 4,
        }
    }
}

/// The application protocol a file declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    /// AP203, configuration controlled design.
    Ap203,
    /// AP214, automotive design.
    Ap214,
    /// AP242, managed model-based 3D engineering: the edition, when the file gives one that is known.
    Ap242(Option<Ap242Edition>),
    /// No schema, or none of the above.
    Unknown,
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Protocol::Ap203 => f.write_str("AP203"),
            Protocol::Ap214 => f.write_str("AP214"),
            Protocol::Ap242(Some(edition)) => write!(f, "AP242 edition {}", edition.number()),
            Protocol::Ap242(None) => f.write_str("AP242"),
            Protocol::Unknown => f.write_str("an unknown protocol"),
        }
    }
}

/// What a file's `FILE_SCHEMA` declares, and what a reader should be told about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declared {
    /// The protocol of the first name that is a known one.
    pub protocol: Protocol,
    /// Why the protocol is not fully known, when it isn't: the file is read anyway.
    pub warning: Option<String>,
}

/// The protocol the `FILE_SCHEMA` names `schemas` declare ([`p21::Exchange::schemas`]): the first of
/// the first [`MAX_SCHEMAS`] names that is AP203, AP214 or AP242, compared without case. An AP242
/// edition comes from the identifier after the name.
///
/// [`p21::Exchange::schemas`]: super::p21::Exchange::schemas
pub fn declared(schemas: &[String]) -> Declared {
    let known = schemas.iter().take(MAX_SCHEMAS).find_map(|schema| {
        let (name, identifier) = split(schema);
        let named = |names: &[&str]| names.iter().any(|n| n.eq_ignore_ascii_case(name));
        if name.eq_ignore_ascii_case(AP242) {
            Some(match identifier {
                None => warned(
                    Protocol::Ap242(None),
                    "the file declares AP242 without its edition; it is read as AP242".into(),
                ),
                Some((id, closed)) => match edition(id).filter(|_| closed) {
                    Some(edition) => known(Protocol::Ap242(Some(edition))),
                    None => warned(
                        Protocol::Ap242(None),
                        format!(
                            "the file declares AP242 with the identifier {{ {} }}, which names no edition known here; it is read as AP242",
                            shown(id)
                        ),
                    ),
                },
            })
        } else if named(&AP203) {
            Some(known(Protocol::Ap203))
        } else if named(&AP214) {
            Some(known(Protocol::Ap214))
        } else {
            None
        }
    });
    known.unwrap_or_else(|| {
        let warning = match schemas.first().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            Some(first) => format!(
                "the file's schema {} is not AP203, AP214 or AP242; it is read as far as it goes",
                shown(first)
            ),
            None => "the file declares no schema; it is read as far as it goes".into(),
        };
        warned(Protocol::Unknown, warning)
    })
}

fn known(protocol: Protocol) -> Declared {
    Declared {
        protocol,
        warning: None,
    }
}

fn warned(protocol: Protocol, warning: String) -> Declared {
    Declared {
        protocol,
        warning: Some(warning),
    }
}

/// A schema's name and its identifier, both trimmed: the text inside the braces, and whether its
/// `}` closes it. An identifier that isn't closed names no edition, but is shown in the warning.
fn split(schema: &str) -> (&str, Option<(&str, bool)>) {
    match schema.split_once('{') {
        Some((name, rest)) => {
            let identifier = match rest.split_once('}') {
                Some((inside, _)) => (inside.trim(), true),
                None => (rest.trim(), false),
            };
            (name.trim(), Some(identifier))
        }
        None => (schema.trim(), None),
    }
}

/// The AP242 edition an identifier's arcs name, if it names one.
fn edition(identifier: &str) -> Option<Ap242Edition> {
    let arcs: Option<Vec<u32>> = identifier
        .split_ascii_whitespace()
        .take(MAX_ARCS + 1)
        .map(|arc| arc.parse().ok())
        .collect();
    match arcs?.as_slice() {
        [1, 0, 10303, 442, 1, 1, 4] => Some(Ap242Edition::First),
        [1, 0, 10303, 442, 3, 1, 4] => Some(Ap242Edition::Second),
        [1, 0, 10303, 442, 4, 1, 4] => Some(Ap242Edition::Third),
        [1, 0, 10303, 442, 7 | 5, 1, 4] => Some(Ap242Edition::Fourth),
        _ => None,
    }
}

/// `text` as a warning repeats it: at most [`MAX_SHOWN`] characters, then `…`.
fn shown(text: &str) -> String {
    let mut chars = text.chars();
    let mut out: String = chars.by_ref().take(MAX_SHOWN).collect();
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(schemas: &[&str]) -> Declared {
        declared(&schemas.iter().map(ToString::to_string).collect::<Vec<_>>())
    }

    fn ap242(id: &str) -> String {
        format!("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF {{ {id} }}")
    }

    #[test]
    fn ap242_editions_by_identifier() {
        for (id, edition) in [
            ("1 0 10303 442 1 1 4", Ap242Edition::First),
            ("1 0 10303 442 3 1 4", Ap242Edition::Second),
            ("1 0 10303 442 4 1 4", Ap242Edition::Third),
            ("1 0 10303 442 7 1 4", Ap242Edition::Fourth),
            ("1 0 10303 442 5 1 4", Ap242Edition::Fourth),
        ] {
            let d = of(&[&ap242(id)]);
            assert_eq!(d, known(Protocol::Ap242(Some(edition))), "{id}");
        }
        assert_eq!(
            Protocol::Ap242(Some(Ap242Edition::Fourth)).to_string(),
            "AP242 edition 4"
        );
    }

    #[test]
    fn ap203_and_ap214_by_name() {
        for name in AP203 {
            assert_eq!(of(&[name]), known(Protocol::Ap203), "{name}");
            let with_id = format!("{name} {{ 1 0 10303 403 2 1 2 }}");
            assert_eq!(of(&[&with_id]), known(Protocol::Ap203), "{with_id}");
        }
        for name in AP214 {
            assert_eq!(of(&[name]), known(Protocol::Ap214), "{name}");
            let with_id = format!("{name} {{ 1 0 10303 214 1 1 1 1 }}");
            assert_eq!(of(&[&with_id]), known(Protocol::Ap214), "{with_id}");
        }
    }

    #[test]
    fn names_ignore_case_and_spaces() {
        let third = known(Protocol::Ap242(Some(Ap242Edition::Third)));
        for schema in [
            "ap242_managed_model_based_3d_engineering_mim_lf { 1 0 10303 442 4 1 4 }",
            "Ap242_Managed_Model_Based_3D_Engineering_Mim_Lf{1 0 10303 442 4 1 4}",
            "  AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF   {   1  0 10303\t442 4 1 4   }  ",
        ] {
            assert_eq!(of(&[schema]), third, "{schema:?}");
        }
        assert_eq!(of(&["  automotive_design  "]), known(Protocol::Ap214));
        assert_eq!(of(&["Config_Control_Design"]), known(Protocol::Ap203));
    }

    #[test]
    fn ap242_without_identifier_is_read_with_a_warning() {
        let d = of(&["AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF"]);
        assert_eq!(d.protocol, Protocol::Ap242(None));
        assert!(d.warning.is_some_and(|w| w.contains("without its edition")));
    }

    #[test]
    fn unknown_identifier_is_read_with_a_warning() {
        for schema in [
            ap242("1 0 10303 442 9 1 4"),
            ap242("1 0 10303 442 1 1"),
            ap242("1 0 10303 442 x 1 4"),
            ap242("1 0 10303 442 99999999999 1 4"),
            ap242(""),
            "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4".into(),
        ] {
            let d = of(&[&schema]);
            assert_eq!(d.protocol, Protocol::Ap242(None), "{schema}");
            assert!(
                d.warning.is_some_and(|w| w.contains("names no edition")),
                "{schema}"
            );
        }
    }

    #[test]
    fn unknown_or_missing_schema_is_read_with_a_warning() {
        for (schemas, says) in [
            (&[][..], "declares no schema"),
            (&[""][..], "declares no schema"),
            (&["X_DESIGN"][..], "X_DESIGN is not AP203"),
        ] {
            let d = of(schemas);
            assert_eq!(d.protocol, Protocol::Unknown, "{schemas:?}");
            assert!(d.warning.is_some_and(|w| w.contains(says)), "{schemas:?}");
        }
    }

    #[test]
    fn the_first_known_schema_decides() {
        assert_eq!(of(&["X", "AUTOMOTIVE_DESIGN"]), known(Protocol::Ap214));
        let both = [
            "AUTOMOTIVE_DESIGN".to_string(),
            ap242("1 0 10303 442 4 1 4"),
        ];
        assert_eq!(declared(&both), known(Protocol::Ap214));
        // A name past the first MAX_SCHEMAS is not looked at.
        let mut late = vec!["X".to_string(); MAX_SCHEMAS];
        late.push("AUTOMOTIVE_DESIGN".into());
        assert_eq!(declared(&late).protocol, Protocol::Unknown);
    }

    /// A small generator for the hostile shapes (no dependency).
    struct Lcg(u64);

    impl Lcg {
        fn below(&mut self, n: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 33) as usize) % n.max(1)
        }
    }

    #[test]
    fn hostile_names_never_panic() {
        let names = [
            AP242,
            "ap242_managed_model_based_3d_engineering_mim_lf",
            "AUTOMOTIVE_DESIGN",
            "",
            " ",
            "{",
            "}",
            "}{",
            "{{}}",
            "é漢字🙂",
        ];
        let many_arcs = "1 ".repeat(10_000);
        let ids = [
            "1 0 10303 442 4 1 4",
            "1 0 10303 442 7 1 4",
            "",
            "{",
            "}",
            "999999999999999999999999999999",
            many_arcs.as_str(),
            "1 0 10303 442 4 1 4 }{ 1",
            "é漢字🙂 1 0",
            "-1 0 10303 442 4 1 4",
        ];
        let long_name = "é".repeat(10_000);
        let mut rng = Lcg(13736);
        let longest = MAX_SHOWN + 200;
        for _ in 0..10_000 {
            let count = rng.below(MAX_SCHEMAS + 4);
            let mut schemas = Vec::new();
            for _ in 0..count {
                let name = match rng.below(12) {
                    10 => long_name.as_str(),
                    11 => many_arcs.as_str(),
                    i => names.get(i).copied().unwrap_or(""),
                };
                let schema = match rng.below(3) {
                    0 => name.to_string(),
                    1 => format!("{name} {{ {} }}", ids[rng.below(ids.len())]),
                    _ => format!("{name}{{{}", ids[rng.below(ids.len())]),
                };
                schemas.push(schema);
            }
            let d = declared(&schemas);
            if let Some(w) = &d.warning {
                assert!(
                    w.chars().count() <= longest,
                    "{} characters",
                    w.chars().count()
                );
            }
            // The first schema decides when it is AP242 with a known identifier.
            if let Some(first) = schemas.first()
                && let (name, Some((id, true))) = split(first)
                && name.eq_ignore_ascii_case(AP242)
                && let Some(edition) = edition(id)
            {
                assert_eq!(d, known(Protocol::Ap242(Some(edition))), "{first:?}");
            }
        }
    }
}
