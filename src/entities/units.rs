//! The drawing units (INSUNITS, DXF group 70): one table of the names and
//! one of the sizes.

/// Human-readable INSUNITS name (DXF group 70 unit codes).
pub(crate) fn insunits_name(code: i16) -> &'static str {
    match code {
        1 => "Inches",
        2 => "Feet",
        3 => "Miles",
        4 => "Millimeters",
        5 => "Centimeters",
        6 => "Meters",
        7 => "Kilometers",
        8 => "Microinches",
        9 => "Mils",
        10 => "Yards",
        11 => "Angstroms",
        12 => "Nanometers",
        13 => "Microns",
        14 => "Decimeters",
        15 => "Decameters",
        16 => "Hectometers",
        17 => "Gigameters",
        18 => "Astronomical Units",
        19 => "Light Years",
        20 => "Parsecs",
        21 => "US Survey Feet",
        22 => "US Survey Inches",
        23 => "US Survey Yards",
        24 => "US Survey Miles",
        0 => "Unitless",
        _ => "Unknown",
    }
}

/// Convert INSUNITS (DXF group 70) to millimetres.
pub(crate) fn insunits_to_mm(code: i16) -> Option<f64> {
    Some(match code {
        1 => 25.4,                       // Inches
        2 => 304.8,                      // Feet
        3 => 1_609_344.0,                // Miles
        4 => 1.0,                        // Millimeters
        5 => 10.0,                       // Centimeters
        6 => 1_000.0,                    // Meters
        7 => 1_000_000.0,                // Kilometers
        8 => 0.000_025_4,                // Microinches
        9 => 0.025_4,                    // Mils
        10 => 914.4,                     // Yards
        11 => 1.0e-7,                    // Angstroms
        12 => 1.0e-6,                    // Nanometers
        13 => 0.001,                     // Microns
        14 => 100.0,                     // Decimeters
        15 => 10_000.0,                  // Decameters
        16 => 100_000.0,                 // Hectometers
        17 => 1.0e12,                    // Gigameters
        18 => 1.495_978_707e14,          // Astronomical Units
        19 => 9.460_730_472_580_8e18,    // Light Years
        20 => 3.085_677_581_491_367e19,  // Parsecs
        21 => 1_200_000.0 / 3_937.0,     // US Survey Feet
        22 => 100_000.0 / 3_937.0,       // US Survey Inches
        23 => 3_600_000.0 / 3_937.0,     // US Survey Yards
        24 => 6_336_000_000.0 / 3_937.0, // US Survey Miles
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{insunits_name, insunits_to_mm};

    #[test]
    fn each_code_has_its_name() {
        let names = [
            "Unitless",
            "Inches",
            "Feet",
            "Miles",
            "Millimeters",
            "Centimeters",
            "Meters",
            "Kilometers",
            "Microinches",
            "Mils",
            "Yards",
            "Angstroms",
            "Nanometers",
            "Microns",
            "Decimeters",
            "Decameters",
            "Hectometers",
            "Gigameters",
            "Astronomical Units",
            "Light Years",
            "Parsecs",
            "US Survey Feet",
            "US Survey Inches",
            "US Survey Yards",
            "US Survey Miles",
        ];
        for (code, name) in names.iter().enumerate() {
            assert_eq!(insunits_name(code as i16), *name, "INSUNITS {code}");
        }
        assert_eq!(insunits_name(25), "Unknown");
        assert_eq!(insunits_name(-1), "Unknown");
    }

    #[test]
    fn only_the_codes_1_to_24_have_a_size() {
        for code in -1..=25 {
            assert_eq!(
                insunits_to_mm(code).is_some(),
                (1..=24).contains(&code),
                "INSUNITS {code}"
            );
        }
    }
}
