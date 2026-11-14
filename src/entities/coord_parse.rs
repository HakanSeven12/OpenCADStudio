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
}
