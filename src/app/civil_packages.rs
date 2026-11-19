//! OpenCAD Studio's own subassembly packages, built in code and written to the user's
//! documents so that the host application finds them by the path the drawing names.

use document::classes::civil::pkt::{
    self, chain, link, new_part_name, parameter_double, parameter_grade, parameter_side, point_delta,
    point_slope, shape, PktPackage, PktSide, PointTargets,
};
use document::classes::civil::PktSource;

/// The folder under the user's documents that holds the packages.
const FOLDER: [&str; 2] = ["OpenCAD Studio", "Subassemblies"];

/// One package of a road's half section.
fn package(name: &str, description: &str, parameters: Vec<pkt::PktParameter>, acts: Vec<pkt::PktActivity>) -> PktPackage {
    let mut p = PktPackage::new_package(name, &new_part_name(&format!("OpenCAD Studio {name} 1")));
    p.description = description.into();
    p.parameters = parameters;
    (p.flowchart, p.start) = chain(acts);
    p
}

/// Index of a package's outer point (its `P2`), where the next package hangs.
pub(super) const OUTER: usize = 1;

/// A road's half section, inside out: a lane from the baseline, a shoulder on the lane's
/// outer point and a side slope on the shoulder's, with the pavement under lane and shoulder.
pub(super) fn road() -> [PktPackage; 3] {
    let none = PointTargets::default;
    // A paved strip from P1 out to P2, with its datum PaveDepth below.
    let paved = |width: &str, slope: &str, start: &'static [&'static str], edge: &'static str, datum: [&'static [&'static str]; 2]| {
        vec![
            point_delta(1, "P1", None, None, "0", "0", start),
            point_slope(2, "P2", "P1", Some(("L1", &["Top", "Pave"])), slope, width, none(), &[edge]),
            point_delta(3, "P3", Some("P2"), Some(("L2", &[])), "0", "[-PaveDepth]", datum[0]),
            point_delta(4, "P4", Some("P1"), None, "0", "[-PaveDepth]", datum[1]),
            link(5, "L3", "P3", "P4", &["Datum"]),
            link(6, "L4", "P4", "P1", &[]),
            shape(7, "S1", &["L1", "L2", "L3", "L4"], &["Pave"]),
        ]
    };
    let depth = || parameter_double("PaveDepth", 0.3, "Pavement depth");
    [
        package(
            "OCSLane",
            "A paved lane from the baseline",
            vec![parameter_side(PktSide::Right), parameter_double("LaneWidth", 3.5, "Lane width"), parameter_grade("LaneSlope", -0.02, "Lane cross slope"), depth()],
            paved("[LaneWidth]", "[LaneSlope]", &["Crown"], "ETW", [&["ETW_Datum"], &["Crown_Datum"]]),
        ),
        package(
            "OCSShoulder",
            "A paved shoulder out from a lane's edge",
            vec![parameter_side(PktSide::Right), parameter_double("ShoulderWidth", 1.0, "Shoulder width"), parameter_grade("ShoulderSlope", -0.04, "Shoulder cross slope"), depth()],
            paved("[ShoulderWidth]", "[ShoulderSlope]", &[], "EPS", [&["EPS_Datum"], &[]]),
        ),
        package(
            "OCSSideSlope",
            "A side slope out to daylight",
            vec![parameter_side(PktSide::Right), parameter_double("SideSlopeWidth", 3.0, "Side slope width"), parameter_grade("SideSlope", -0.5, "Side slope grade")],
            vec![
                point_delta(1, "P1", None, None, "0", "0", &[]),
                point_slope(2, "P2", "P1", Some(("L1", &["Top", "Daylight"])), "[SideSlope]", "[SideSlopeWidth]", none(), &["Daylight"]),
            ],
        ),
    ]
}

/// Writes `pkg` to the user's package folder (when the file there differs) and returns the
/// texts a subassembly names it by.
pub(super) fn install(pkg: &PktPackage) -> Result<PktSource, String> {
    let bytes = pkt::write_pkt(pkg).map_err(|e| e.to_string())?;
    let file = format!("{}.pkt", pkg.name);
    let mut dir = crate::sys::documents_dir().ok_or("no documents folder")?;
    dir.extend(FOLDER);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(&file);
    if std::fs::read(&path).ok().as_deref() != Some(bytes.as_slice()) {
        std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    }
    let profile = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(std::path::PathBuf::from);
    let host = host_path(&path, profile.as_deref());
    Ok(PktSource { library: format!("[ \"{}\" ]", host.replace('\\', "\\\\")), path: host })
}

/// How the drawing names the package at `path`: the place it was written to, with
/// `%USERPROFILE%` for the user's folder when the path lies under it (the documents folder
/// may be redirected, so it is not assumed to be `%USERPROFILE%\Documents`), in the host's
/// backslash form.
fn host_path(path: &std::path::Path, profile: Option<&std::path::Path>) -> String {
    let parts = |p: &std::path::Path| p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>();
    match profile.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => std::iter::once("%USERPROFILE%".to_string()).chain(parts(rest)).collect::<Vec<_>>().join("\\"),
        None => path.to_string_lossy().replace('/', "\\"),
    }
}

#[cfg(test)]
mod tests {
    use super::host_path;
    use std::path::Path;

    #[test]
    fn names_the_written_folder_under_the_profile() {
        let home = Path::new("/home/u");
        assert_eq!(host_path(&home.join("OneDrive/Documents/OpenCAD Studio/Subassemblies/OCSRoad.pkt"), Some(home)), "%USERPROFILE%\\OneDrive\\Documents\\OpenCAD Studio\\Subassemblies\\OCSRoad.pkt");
        assert_eq!(host_path(Path::new("/data/docs/OCSRoad.pkt"), Some(home)), "\\data\\docs\\OCSRoad.pkt");
    }
}
