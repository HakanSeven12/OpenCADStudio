use super::*;
use hayro::hayro_interpret::TransformExt;
use std::sync::Arc;

fn make_pdf(version: &str, page_entries: &str, extra_objects: &[String]) -> Pdf {
    let objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] {page_entries} >>"),
    ]
    .into_iter()
    .chain(extra_objects.iter().cloned())
    .collect::<Vec<_>>();
    let mut bytes = format!("%PDF-{version}\n").into_bytes();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend(format!("{} 0 obj\n{}\nendobj\n", index + 1, object).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    bytes.extend(b"0000000000 65535 f \n");
    for offset in offsets {
        bytes.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    Pdf::new(Arc::new(bytes)).unwrap()
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} differs from {expected} by more than {tolerance}"
    );
}

fn check_placement(placement: GeoPlacement, insertion: [f64; 2], scales: [f64; 2]) {
    close(placement.insertion[0], insertion[0], 1e-5);
    close(placement.insertion[1], insertion[1], 1e-5);
    close(placement.x_scale, scales[0], 1e-5);
    close(placement.y_scale, scales[1], 1e-5);
}

fn model_points(_crs: &CrsDefinition) -> Vec<([f64; 2], [f64; 2])> {
    let origin = [-742_925.271_389_167_2, -1_043_020.310_123_933_5];
    [[0.0, 0.0], [10.0, 0.0], [0.0, 20.0], [10.0, 20.0]]
        .into_iter()
        .map(|source| {
            let target = [origin[0] + 2.0 * source[0], origin[1] + 3.0 * source[1]];
            (source, target)
        })
        .collect()
}

fn geographic_gpts(crs: &CrsDefinition) -> String {
    model_points(crs)
        .into_iter()
        .map(|(_, target)| {
            let (lon, lat) = crs.to_base_geographic(target[0], target[1]).unwrap();
            format!("{lat:.15} {lon:.15}")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn projected_gpts(crs: &CrsDefinition) -> String {
    model_points(crs)
        .into_iter()
        .map(|(_, target)| format!("{:.15} {:.15}", target[0], target[1]))
        .collect::<Vec<_>>()
        .join(" ")
}

fn viewport(gcs: String, gpts: String, extra_measure: &str) -> String {
    format!(
        "<< /Type /Viewport /BBox [0 0 10 20] /Measure << /Type /Measure /Subtype /GEO /GCS {gcs} /LPTS [0 0 1 0 0 1 1 1] /GPTS [{gpts}] {extra_measure} >> >>"
    )
}

#[test]
fn adobe_projected_gpts_places_s_jtsk_without_wgs84() {
    let crs = CrsDefinition::from_epsg(5514).unwrap();
    let vp = viewport(
        "<< /Type /PROJCS /EPSG 5514 >>".to_string(),
        geographic_gpts(&crs),
        "",
    );
    let pdf = make_pdf("1.7", &format!("/VP [{vp}]"), &[]);
    let placement = placement_for_pdf_page(&pdf, 0, &crs, 1000.0).unwrap();
    check_placement(
        placement,
        [-742_925.271_389_167_2, -1_043_020.310_123_933_5],
        [144.0, 216.0],
    );
}

#[test]
fn iso_projected_gpts_uses_easting_northing_and_rejects_pcsm() {
    let crs = CrsDefinition::from_epsg(5514).unwrap();
    let fitted = fit_affine(&model_points(&crs)).unwrap();
    assert_eq!(
        fitted.as_coeffs(),
        [
            2.0,
            0.0,
            0.0,
            3.0,
            -742_925.271_389_167_2,
            -1_043_020.310_123_933_5
        ]
    );
    let gcs = "<< /Type /PROJCS /EPSG 5514 >>".to_string();
    let vp = viewport(gcs.clone(), projected_gpts(&crs), "");
    let pdf = make_pdf("2.0", &format!("/VP [{vp}]"), &[]);
    check_placement(
        placement_for_pdf_page(&pdf, 0, &crs, 1000.0).unwrap(),
        [-742_925.271_389_167_2, -1_043_020.310_123_933_5],
        [144.0, 216.0],
    );
    let vp = viewport(gcs, projected_gpts(&crs), "/PCSM [1 0 0 0 0 1 0 0 0 0 1 0]");
    let pdf = make_pdf("2.0", &format!("/VP [{vp}]"), &[]);
    assert!(placement_for_pdf_page(&pdf, 0, &crs, 1000.0).is_err());
}

#[test]
fn base_geographic_gcs_matches_the_projected_drawing_crs() {
    let crs = CrsDefinition::from_epsg(5514).unwrap();
    let vp = viewport(
        "<< /Type /GEOGCS /EPSG 4156 >>".to_string(),
        geographic_gpts(&crs),
        "",
    );
    let pdf = make_pdf("2.0", &format!("/VP {vp}"), &[]);
    assert!(placement_for_pdf_page(&pdf, 0, &crs, 1000.0).is_ok());
    let mismatched = CrsDefinition::from_epsg(4326).unwrap();
    assert!(placement_for_pdf_page(&pdf, 0, &mismatched, 1000.0).is_err());
}

#[test]
fn iso_display_dcs_does_not_change_geographic_gpts_or_enable_pcsm() {
    let crs = CrsDefinition::from_epsg(5514).unwrap();
    let vp = viewport(
        "<< /Type /GEOGCS /EPSG 4156 >>".to_string(),
        geographic_gpts(&crs),
        "/DCS << /Type /PROJCS /EPSG 5514 >> /PCSM [1 0 0 0 0 1 0 0 0 0 1 0]",
    );
    let pdf = make_pdf("2.0", &format!("/VP [{vp}]"), &[]);
    check_placement(
        placement_for_pdf_page(&pdf, 0, &crs, 1000.0).unwrap(),
        [-742_925.271_389_167_2, -1_043_020.310_123_933_5],
        [144.0, 216.0],
    );
}

#[test]
fn indirect_viewport_and_lgit_arrays_are_supported_but_ambiguity_is_not() {
    let crs = CrsDefinition::from_epsg(5514).unwrap();
    let vp = viewport(
        "<< /Type /PROJCS /EPSG 5514 >>".to_string(),
        geographic_gpts(&crs),
        "",
    );
    let pdf = make_pdf("1.7", "/VP 4 0 R", std::slice::from_ref(&vp));
    assert!(placement_for_pdf_page(&pdf, 0, &crs, 1000.0).is_ok());
    let pdf = make_pdf("1.7", &format!("/VP [{vp} {vp}]"), &[]);
    assert!(placement_for_pdf_page(&pdf, 0, &crs, 1000.0).is_err());
    let pdf = make_pdf(
        "1.7",
        &format!("/VP [{vp}] /LGIDict << /Type /LGIDict >>"),
        &[],
    );
    assert!(placement_for_pdf_page(&pdf, 0, &crs, 1000.0).is_err());
}

#[test]
fn lgit_gpts_takes_precedence_over_lower_precision_standard_values() {
    let crs = CrsDefinition::from_epsg(5514).unwrap();
    let vp = viewport(
        "<< /Type /PROJCS /EPSG 5514 >>".to_string(),
        geographic_gpts(&crs),
        "",
    );
    // Replace the standard arrays with unusable values while supplying the
    // precise compatibility arrays actually used for placement.
    let vp = vp.replace(
        "/LPTS [0 0 1 0 0 1 1 1] /GPTS",
        "/LPTS [0 0 0 0 0 0] /lgit:LPTS [0 0 1 0 0 1 1 1] /lgit:GPTS",
    );
    let pdf = make_pdf("1.7", &format!("/VP [{vp}]"), &[]);
    assert!(placement_for_pdf_page(&pdf, 0, &crs, 1000.0).is_ok());
}

fn legacy_wkt(crs: &CrsDefinition) -> String {
    let wkt = crs.wkt().unwrap();
    let parsed = CrsDefinition::from_wkt_parameters(wkt).unwrap();
    assert!(
        parsed.equivalent_to(crs),
        "authored: {}\nregistry: {}",
        parsed.proj4(),
        crs.proj4()
    );
    format!("<< /WKT ({wkt}) >>")
}

#[test]
fn legacy_ctm_accepts_numeric_strings_and_precedes_registration() {
    let crs = CrsDefinition::from_epsg(5514).unwrap();
    let projection = legacy_wkt(&crs);
    let lgi = format!(
        "<< /Type /LGIDict /Projection {projection} /Units /M /CTM [(2) (0) (0) (3) (-742925.2713891672) (-1043020.3101239335)] /Registration [0 0 0 0 1 0 1000000 0 0 1 0 1000000] >>"
    );
    let pdf = make_pdf("1.7", &format!("/LGIDict {lgi}"), &[]);
    check_placement(
        placement_for_pdf_page(&pdf, 0, &crs, 1000.0).unwrap(),
        [-742_925.271_389_167_2, -1_043_020.310_123_933_5],
        [144.0, 216.0],
    );
}

#[test]
fn legacy_registration_fits_affine_and_requires_three_non_collinear_points() {
    let crs = CrsDefinition::from_epsg(5514).unwrap();
    let projection = legacy_wkt(&crs);
    let registration = model_points(&crs)
        .into_iter()
        .map(|(source, target)| {
            format!(
                "{} {} {:.15} {:.15}",
                source[0], source[1], target[0], target[1]
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let lgi =
        format!("<< /Type /LGIDict /Projection {projection} /Registration [{registration}] >>");
    let pdf = make_pdf("1.7", &format!("/LGIDict {lgi}"), &[]);
    let parsed = required_numbers(
        &pdf.pages()
            .first()
            .unwrap()
            .raw()
            .get::<Dict<'_>>(b"LGIDict")
            .unwrap(),
        b"Registration",
        b"Registration",
    )
    .unwrap();
    assert_eq!(parsed[2], -742_925.271_389_167_2);
    assert_eq!(parsed[3], -1_043_020.310_123_933_5);
    check_placement(
        placement_for_pdf_page(&pdf, 0, &crs, 1000.0).unwrap(),
        [-742_925.271_389_167_2, -1_043_020.310_123_933_5],
        [144.0, 216.0],
    );
    let nested = model_points(&crs)
        .into_iter()
        .map(|(source, target)| {
            format!(
                "[{} {} {:.15} {:.15}]",
                source[0], source[1], target[0], target[1]
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let lgi = format!("<< /Type /LGIDict /Projection {projection} /Registration [{nested}] >>");
    let pdf = make_pdf("1.7", &format!("/LGIDict {lgi}"), &[]);
    check_placement(
        placement_for_pdf_page(&pdf, 0, &crs, 1000.0).unwrap(),
        [-742_925.271_389_167_2, -1_043_020.310_123_933_5],
        [144.0, 216.0],
    );
    let short =
        format!("<< /Type /LGIDict /Projection {projection} /Registration [0 0 0 0 1 0 2 0] >>");
    let pdf = make_pdf("1.7", &format!("/LGIDict {short}"), &[]);
    assert!(placement_for_pdf_page(&pdf, 0, &crs, 1000.0).is_err());
}

#[test]
fn legacy_geographic_registration_projects_each_point_but_ctm_falls_back() {
    let crs = CrsDefinition::from_epsg(5514).unwrap();
    let projection = "<< /EPSG 4156 >>";
    let registration = model_points(&crs)
        .into_iter()
        .map(|(source, target)| {
            let (lon, lat) = crs.to_base_geographic(target[0], target[1]).unwrap();
            format!("[{} {} {lon:.15} {lat:.15}]", source[0], source[1])
        })
        .collect::<Vec<_>>()
        .join(" ");
    let lgi =
        format!("<< /Type /LGIDict /Projection {projection} /Registration [{registration}] >>");
    let pdf = make_pdf("1.7", &format!("/LGIDict {lgi}"), &[]);
    check_placement(
        placement_for_pdf_page(&pdf, 0, &crs, 1000.0).unwrap(),
        [-742_925.271_389_167_2, -1_043_020.310_123_933_5],
        [144.0, 216.0],
    );
    let ctm = format!("<< /Type /LGIDict /Projection {projection} /CTM [2 0 0 3 14 50] >>");
    let pdf = make_pdf("1.7", &format!("/LGIDict {ctm}"), &[]);
    assert!(placement_for_pdf_page(&pdf, 0, &crs, 1000.0).is_err());
}

#[test]
fn epsg_label_cannot_override_a_conflicting_wkt_operation() {
    let crs = CrsDefinition::from_epsg(5514).unwrap();
    let altered = CrsDefinition::from_epsg(3857)
        .unwrap()
        .wkt()
        .unwrap()
        .replace("3857", "5514");
    let vp = viewport(
        format!("<< /Type /PROJCS /EPSG 5514 /WKT ({altered}) >>"),
        geographic_gpts(&crs),
        "",
    );
    let pdf = make_pdf("1.7", &format!("/VP [{vp}]"), &[]);
    assert!(placement_for_pdf_page(&pdf, 0, &crs, 1000.0).is_err());
}

#[test]
fn source_metres_convert_explicitly_to_drawing_millimetres() {
    let crs = CrsDefinition::from_epsg(5514).unwrap();
    let projection = legacy_wkt(&crs);
    let lgi = format!(
        "<< /Type /LGIDict /Projection {projection} /CTM [2 0 0 3 -742925.2713891672 -1043020.3101239335] >>"
    );
    let pdf = make_pdf("1.7", &format!("/LGIDict {lgi}"), &[]);
    check_placement(
        placement_for_pdf_page(&pdf, 0, &crs, 1.0).unwrap(),
        [
            -742_925.271_389_167_2 * 1000.0,
            -1_043_020.310_123_933_5 * 1000.0,
        ],
        [144_000.0, 216_000.0],
    );
}

#[test]
fn legacy_utm_whitelist_works_and_unknown_units_or_shear_fall_back() {
    let crs = CrsDefinition::from_epsg(32633).unwrap();
    let lgi = "<< /Type /LGIDict /Projection << /Type /Projection /ProjectionType (UT) /Zone 33 /Hemisphere (N) /Datum (WE) >> /CTM [1 0 0 1 500000 0] >>";
    let pdf = make_pdf("1.7", &format!("/LGIDict {lgi}"), &[]);
    check_placement(
        placement_for_pdf_page(&pdf, 0, &crs, 1000.0).unwrap(),
        [500_000.0, 0.0],
        [72.0, 72.0],
    );
    let geographic = CrsDefinition::from_epsg(4326).unwrap();
    let lgi = "<< /Type /LGIDict /Projection << /Type /Projection /ProjectionType (GEOGRAPHIC) /Datum (WE) >> /CTM [2 0 0 3 14 50] >>";
    let pdf = make_pdf("1.7", &format!("/LGIDict {lgi}"), &[]);
    check_placement(
        placement_for_pdf_page(&pdf, 0, &geographic, 1000.0).unwrap(),
        [14.0, 50.0],
        [144.0, 216.0],
    );
    let feet = "<< /Type /LGIDict /Projection << /Type /Projection /ProjectionType (UT) /Zone 33 /Hemisphere (N) /Datum (WGE) >> /Units /FT /CTM [1 0 0 1 500000 0] >>";
    let pdf = make_pdf("1.7", &format!("/LGIDict {feet}"), &[]);
    assert!(placement_for_pdf_page(&pdf, 0, &crs, 1000.0).is_err());
    let shear = "<< /Type /LGIDict /Projection << /Type /Projection /ProjectionType (UT) /Zone 33 /Hemisphere (N) /Datum (WE) >> /CTM [1 0 0.2 1 500000 0] >>";
    let pdf = make_pdf("1.7", &format!("/LGIDict {shear}"), &[]);
    assert!(placement_for_pdf_page(&pdf, 0, &crs, 1000.0).is_err());
}

#[test]
fn crop_rotation_and_user_unit_compose_without_an_extra_scale() {
    let crs = CrsDefinition::from_epsg(5514).unwrap();
    let projection = legacy_wkt(&crs);
    for rotate in [0, 90, 180, 270] {
        let lgi = format!(
            "<< /Type /LGIDict /Projection {projection} /CTM [2 0 0 3 -742925.2713891672 -1043020.3101239335] >>"
        );
        let pdf = make_pdf(
            "1.7",
            &format!("/CropBox [10 20 110 70] /Rotate {rotate} /UserUnit 2 /LGIDict {lgi}"),
            &[],
        );
        let placement = placement_for_pdf_page(&pdf, 0, &crs, 1000.0).unwrap();
        let page = pdf.pages().first().unwrap();
        let expected = Affine::new([
            2.0,
            0.0,
            0.0,
            3.0,
            -742_925.271_389_167_2,
            -1_043_020.310_123_933_5,
        ]) * page.initial_transform(false).to_kurbo().inverse()
            * Affine::scale(72.0);
        let (sin, cos) = placement.rotation.sin_cos();
        let actual = Affine::new([
            placement.x_scale * cos,
            placement.x_scale * sin,
            -placement.y_scale * sin,
            placement.y_scale * cos,
            placement.insertion[0],
            placement.insertion[1],
        ]);
        for (actual, expected) in actual.as_coeffs().into_iter().zip(expected.as_coeffs()) {
            close(actual, expected, 1e-5);
        }
    }
}
