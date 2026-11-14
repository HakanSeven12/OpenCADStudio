//! Typed coordinate input: the forms a point prompt accepts (`x,y`, `@x,y`,
//! `#x,y`, `distance<angle` and the 3D variants), parsed into a point and how
//! to interpret it.

/// How a typed coordinate should be interpreted relative to the last
/// input point.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CoordKind {
    /// `@x,y` prefix — offset from the last input point.
    Relative,
    /// `#x,y` prefix — world/UCS absolute, overriding DYN.
    Absolute,
    /// No prefix — the caller decides (DYN on → relative, off → absolute).
    Default,
}

/// Parse a typed coordinate string into a local Vec3 plus its interpretation.
/// Accepts Cartesian, polar, cylindrical and spherical forms:
/// `x,y`, `x,y,z`, `distance<angle`, `distance<angle,z`, and
/// `distance<angle<elevation`.
/// A leading `@` marks the value relative to the last point; a leading
/// `#` forces absolute. Separators: comma or semicolon.
///
/// Each number is read by the same pair that writes it, so anything the
/// drawing displays can be typed straight back: `5'-6 1/2"`, `9 1/2`,
/// `@72'8"<n45d20'6"e`. Angles after `<` are directions, and so are counted
/// from the drawing's zero and run the way it says.
pub(crate) fn parse_coord(text: &str) -> Option<(glam::DVec3, CoordKind)> {
    use crate::entities::common::{parse_direction, parse_length};
    let trimmed = text.trim();
    let (kind, rest) = if let Some(r) = trimmed.strip_prefix('@') {
        (CoordKind::Relative, r)
    } else if let Some(r) = trimmed.strip_prefix('#') {
        (CoordKind::Absolute, r)
    } else {
        (CoordKind::Default, trimmed)
    };
    if let Some((distance, angles)) = rest.split_once('<') {
        let distance = parse_length(distance)?;
        if let Some((azimuth, elevation)) = angles.split_once('<') {
            let azimuth = parse_direction(azimuth)?;
            // The rise out of the plane is a size, not a compass direction, so
            // it is not counted from the drawing's zero.
            let elevation = crate::entities::common::parse_angle(elevation)?;
            let horizontal = distance * elevation.cos();
            return Some((
                glam::DVec3::new(
                    horizontal * azimuth.cos(),
                    horizontal * azimuth.sin(),
                    distance * elevation.sin(),
                ),
                kind,
            ));
        }
        // A cylindrical `distance<angle,z` splits on the comma — but a
        // surveyor's bearing has none, so nothing here can eat one.
        let (angle, z) = match angles.split_once([',', ';']) {
            Some((angle, z)) => (parse_direction(angle)?, parse_length(z)?),
            None => (parse_direction(angles)?, 0.0),
        };
        return Some((
            glam::DVec3::new(distance * angle.cos(), distance * angle.sin(), z),
            kind,
        ));
    }
    let parts: Vec<f64> = rest
        .split([',', ';'])
        .map(parse_length)
        .collect::<Option<Vec<f64>>>()?;
    match parts.as_slice() {
        [x, y] => Some((glam::DVec3::new(*x, *y, 0.0), kind)),
        [x, y, z] => Some((glam::DVec3::new(*x, *y, *z), kind)),
        _ => None,
    }
}

#[cfg(test)]
mod coordinate_parsing_tests {
    use super::*;

    #[test]
    fn parses_all_coordinate_forms() {
        let close = |a: glam::DVec3, b: glam::DVec3| (a - b).length() < 1e-9;
        assert!(close(
            parse_coord("1,2").unwrap().0,
            glam::dvec3(1.0, 2.0, 0.0)
        ));
        assert!(close(
            parse_coord("1,2,3").unwrap().0,
            glam::dvec3(1.0, 2.0, 3.0)
        ));
        assert!(close(
            parse_coord("10<90").unwrap().0,
            glam::dvec3(0.0, 10.0, 0.0)
        ));
        assert!(close(
            parse_coord("10<90,4").unwrap().0,
            glam::dvec3(0.0, 10.0, 4.0)
        ));
        assert!(close(
            parse_coord("10<0<30").unwrap().0,
            glam::dvec3(5.0 * 3.0_f64.sqrt(), 0.0, 5.0)
        ));
        assert_eq!(parse_coord("@10<0").unwrap().1, CoordKind::Relative);
        assert_eq!(parse_coord("#1,2").unwrap().1, CoordKind::Absolute);
    }

    use crate::entities::common::{set_unit_context, unit_context, UnitContext};
    use glam::{dvec3, DVec3};
    use std::f64::consts::FRAC_PI_2;

    /// Parse `text`, which has to be accepted, and check the point and how it
    /// is to be interpreted.
    #[track_caller]
    fn assert_coord(text: &str, expected: DVec3, kind: CoordKind) {
        let Some((point, got_kind)) = parse_coord(text) else {
            panic!("{text:?} was rejected");
        };
        assert!(
            (point - expected).length() < 1e-9,
            "{text:?}: expected {expected:?}, got {point:?}"
        );
        assert_eq!(got_kind, kind, "{text:?}");
    }

    #[track_caller]
    fn assert_rejected(text: &str) {
        assert_eq!(parse_coord(text), None, "{text:?} should be rejected");
    }

    /// Changes this thread's unit context and puts the old one back when
    /// dropped, so a failing assertion cannot leak settings into another test.
    struct UnitsGuard(UnitContext);

    impl Drop for UnitsGuard {
        fn drop(&mut self) {
            set_unit_context(self.0);
        }
    }

    fn with_units(change: impl FnOnce(&mut UnitContext)) -> UnitsGuard {
        let saved = unit_context();
        let mut ctx = saved;
        change(&mut ctx);
        set_unit_context(ctx);
        UnitsGuard(saved)
    }

    #[test]
    fn the_prefix_sets_the_kind_and_leaves_the_value_alone() {
        for (prefix, kind) in [
            ("", CoordKind::Default),
            ("@", CoordKind::Relative),
            ("#", CoordKind::Absolute),
        ] {
            assert_coord(&format!("{prefix}3,4"), dvec3(3.0, 4.0, 0.0), kind);
            assert_coord(&format!("{prefix}3,4,5"), dvec3(3.0, 4.0, 5.0), kind);
            assert_coord(&format!("{prefix}10<90"), dvec3(0.0, 10.0, 0.0), kind);
            assert_coord(&format!("{prefix}10<90,4"), dvec3(0.0, 10.0, 4.0), kind);
            assert_coord(
                &format!("{prefix}10<0<30"),
                dvec3(5.0 * 3.0_f64.sqrt(), 0.0, 5.0),
                kind,
            );
        }
    }

    #[test]
    fn a_semicolon_separates_like_a_comma() {
        assert_coord("1;2", dvec3(1.0, 2.0, 0.0), CoordKind::Default);
        assert_coord("1;2;3", dvec3(1.0, 2.0, 3.0), CoordKind::Default);
        assert_coord("1,2;3", dvec3(1.0, 2.0, 3.0), CoordKind::Default);
        assert_coord("10<90;4", dvec3(0.0, 10.0, 4.0), CoordKind::Default);
        assert_coord("@1;2", dvec3(1.0, 2.0, 0.0), CoordKind::Relative);
    }

    /// Inside a coordinate the comma separates the axes, so it is never a
    /// decimal mark.
    #[test]
    fn a_comma_is_never_a_decimal_mark() {
        assert_coord("1,5", dvec3(1.0, 5.0, 0.0), CoordKind::Default);
        assert_coord("1.5,2.25", dvec3(1.5, 2.25, 0.0), CoordKind::Default);
    }

    #[test]
    fn whitespace_around_the_parts_is_ignored() {
        assert_coord("  1,2  ", dvec3(1.0, 2.0, 0.0), CoordKind::Default);
        assert_coord(" 1 , 2 , 3 ", dvec3(1.0, 2.0, 3.0), CoordKind::Default);
        assert_coord("  @1,2", dvec3(1.0, 2.0, 0.0), CoordKind::Relative);
        assert_coord("@ 1,2", dvec3(1.0, 2.0, 0.0), CoordKind::Relative);
        assert_coord(" 10 < 90 , 4 ", dvec3(0.0, 10.0, 4.0), CoordKind::Default);
        assert_coord(
            "10 < 0 < 30",
            dvec3(5.0 * 3.0_f64.sqrt(), 0.0, 5.0),
            CoordKind::Default,
        );
    }

    #[test]
    fn signed_and_scientific_numbers_are_accepted() {
        assert_coord("-1,-2", dvec3(-1.0, -2.0, 0.0), CoordKind::Default);
        assert_coord("-1,-2,-3", dvec3(-1.0, -2.0, -3.0), CoordKind::Default);
        assert_coord("@-1.5,2", dvec3(-1.5, 2.0, 0.0), CoordKind::Relative);
        assert_coord("10<-90", dvec3(0.0, -10.0, 0.0), CoordKind::Default);
        assert_coord("10<90,-4", dvec3(0.0, 10.0, -4.0), CoordKind::Default);
        // A negative elevation points below the plane.
        assert_coord(
            "10<0<-30",
            dvec3(5.0 * 3.0_f64.sqrt(), 0.0, -5.0),
            CoordKind::Default,
        );
        assert_coord("+1,+2", dvec3(1.0, 2.0, 0.0), CoordKind::Default);
        assert_coord("1e3,2.5e-1", dvec3(1000.0, 0.25, 0.0), CoordKind::Default);
        // A negative distance runs the opposite way along the same line.
        assert_coord("-10<0", dvec3(-10.0, 0.0, 0.0), CoordKind::Default);
    }

    /// Feet and inches read as inches, so what an architectural readout shows
    /// can be typed straight back.
    #[test]
    fn feet_and_inches_are_read_inside_a_coordinate() {
        assert_coord("5'-6 1/2\",3'", dvec3(66.5, 36.0, 0.0), CoordKind::Default);
        assert_coord("9 1/2,1/4", dvec3(9.5, 0.25, 0.0), CoordKind::Default);
        assert_coord("@1',2',3'", dvec3(12.0, 24.0, 36.0), CoordKind::Relative);
        assert_coord("5'6\"<90", dvec3(0.0, 66.0, 0.0), CoordKind::Default);
        assert_coord("10<90,1'", dvec3(0.0, 10.0, 12.0), CoordKind::Default);
    }

    /// The cylindrical form splits on the comma after the angle; a bearing
    /// has none, so it is never cut in two.
    #[test]
    fn a_surveyors_bearing_is_a_direction() {
        let north_45_east = 45.0_f64.to_radians();
        assert_coord(
            "10<n45de",
            dvec3(10.0 * north_45_east.cos(), 10.0 * north_45_east.sin(), 0.0),
            CoordKind::Default,
        );
        // S30dW is 30 degrees west of south: 240 degrees from east.
        let south_30_west = 240.0_f64.to_radians();
        assert_coord(
            "10<S30dW",
            dvec3(10.0 * south_30_west.cos(), 10.0 * south_30_west.sin(), 0.0),
            CoordKind::Default,
        );
        assert_coord("10<n", dvec3(0.0, 10.0, 0.0), CoordKind::Default);
        assert_coord("10<w", dvec3(-10.0, 0.0, 0.0), CoordKind::Default);
        assert_coord(
            "10<n45de,4",
            dvec3(10.0 * north_45_east.cos(), 10.0 * north_45_east.sin(), 4.0),
            CoordKind::Default,
        );

        // The example from the doc comment: 72'8" is 872 inches, and
        // N45d20'6"E is 90 - 45.335 = 44.665 degrees from east.
        let bearing = (90.0_f64 - (45.0 + 20.0 / 60.0 + 6.0 / 3600.0)).to_radians();
        assert_coord(
            "@72'8\"<n45d20'6\"e",
            dvec3(872.0 * bearing.cos(), 872.0 * bearing.sin(), 0.0),
            CoordKind::Relative,
        );
    }

    #[test]
    fn angle_marks_choose_the_unit() {
        assert_coord("10<100g", dvec3(0.0, 10.0, 0.0), CoordKind::Default);
        assert_coord(
            "10<1.5707963267948966r",
            dvec3(0.0, 10.0, 0.0),
            CoordKind::Default,
        );
        assert_coord("10<90d", dvec3(0.0, 10.0, 0.0), CoordKind::Default);
        assert_coord("10<89d60'", dvec3(0.0, 10.0, 0.0), CoordKind::Default);
    }

    #[test]
    fn input_that_is_not_a_coordinate_is_rejected() {
        for text in [
            "",
            "   ",
            "@",
            "#",
            "5",
            "@5",
            "1,2,3,4",
            "1,,2",
            ",",
            "1,",
            ",2",
            "abc",
            "1,abc",
            "10<abc",
            "10<",
            "<90",
            "abc<90",
            "10<90,",
            "10<90,abc",
            "10<90,4,5",
            "10<0<",
            "10<0<abc",
            "10<0<30,5",
            "10<<30",
            "1,2<90",
            "1,2,3<4",
            "@@1,2",
            "#@1,2",
        ] {
            assert_rejected(text);
        }
    }

    /// Values past what geometry can hold are refused, not passed on.
    #[test]
    fn out_of_range_values_are_rejected() {
        for text in ["inf,1", "1,nan", "1e16,0", "1e16<0", "10<inf", "10<1e18"] {
            assert_rejected(text);
        }
        assert_coord("1e15,0", dvec3(1.0e15, 0.0, 0.0), CoordKind::Default);
    }

    /// A direction after `<` is counted from the drawing's zero and runs the
    /// way the drawing says.
    #[test]
    fn directions_follow_the_drawings_angle_base_and_direction() {
        {
            let _units = with_units(|ctx| ctx.angbase = FRAC_PI_2);
            assert_coord("10<0", dvec3(0.0, 10.0, 0.0), CoordKind::Default);
            assert_coord("10<90", dvec3(-10.0, 0.0, 0.0), CoordKind::Default);
            assert_coord("@10<0,4", dvec3(0.0, 10.0, 4.0), CoordKind::Relative);
        }
        {
            let _units = with_units(|ctx| ctx.angdir_cw = true);
            assert_coord("10<90", dvec3(0.0, -10.0, 0.0), CoordKind::Default);
            assert_coord("10<0", dvec3(10.0, 0.0, 0.0), CoordKind::Default);
            // The cylindrical form turns the same way.
            assert_coord("10<90,4", dvec3(0.0, -10.0, 4.0), CoordKind::Default);
        }
        {
            let _units = with_units(|ctx| {
                ctx.angbase = FRAC_PI_2;
                ctx.angdir_cw = true;
            });
            assert_coord("10<90", dvec3(10.0, 0.0, 0.0), CoordKind::Default);
        }
        // The guards have put the defaults back.
        assert_coord("10<90", dvec3(0.0, 10.0, 0.0), CoordKind::Default);
    }

    /// The rise out of the plane is a size, not a compass direction: it is
    /// not counted from the drawing's zero and does not turn with it.
    #[test]
    fn the_spherical_elevation_ignores_the_angle_base_and_direction() {
        let _units = with_units(|ctx| {
            ctx.angbase = FRAC_PI_2;
            ctx.angdir_cw = true;
        });
        // Azimuth 0 is the drawing's zero, north here; elevation is still 30
        // degrees up.
        assert_coord(
            "10<0<30",
            dvec3(0.0, 5.0 * 3.0_f64.sqrt(), 5.0),
            CoordKind::Default,
        );
        // An azimuth off the axes shows the direction it is counted in: 60
        // degrees clockwise from north is 30 degrees from east. The length in
        // the plane is still 10·cos 30.
        let in_plane = 5.0 * 3.0_f64.sqrt();
        let from_east = 30.0_f64.to_radians();
        assert_coord(
            "10<60<30",
            dvec3(in_plane * from_east.cos(), in_plane * from_east.sin(), 5.0),
            CoordKind::Default,
        );
    }

    /// A bare number after `<` is read in the drawing's angle unit.
    #[test]
    fn a_bare_angle_uses_the_drawings_angle_unit() {
        {
            let _units = with_units(|ctx| ctx.aunits = 2);
            assert_coord("10<100", dvec3(0.0, 10.0, 0.0), CoordKind::Default);
        }
        {
            let _units = with_units(|ctx| ctx.aunits = 3);
            assert_coord(
                "10<1.5707963267948966",
                dvec3(0.0, 10.0, 0.0),
                CoordKind::Default,
            );
        }
        assert_coord("10<90", dvec3(0.0, 10.0, 0.0), CoordKind::Default);
    }
}
