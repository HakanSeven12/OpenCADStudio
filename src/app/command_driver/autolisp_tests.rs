#[cfg(test)]
mod tests {
    use crate::app::OpenCADStudio;
    use codec::entities::{Circle, Line, LwPolyline, LwVertex};
    use codec::types::Vector3;
    use codec::{EntityType, Handle};
    use serde_json::json;

    #[derive(Debug, Clone)]
    pub struct TestRecord {
        pub category: &'static str,
        pub command: &'static str,
        pub option: &'static str,
        pub cmd_input: String,
        pub status: &'static str, // PASS, DROPPED_TOKENS, STRANDED, ERROR, UNEXPECTED
        pub added_entities: i64,
        pub unconsumed: Vec<String>,
        pub error: Option<String>,
        pub details: String,
    }

    fn run_test(
        app: &mut OpenCADStudio,
        cat: &'static str,
        cmd: &'static str,
        opt: &'static str,
        input: &str,
    ) -> TestRecord {
        let op = json!({
            "op": "run",
            "cmd": input
        });
        let res = app.automation_op(&op.to_string());
        
        let ok = res["ok"].as_bool().unwrap_or(false);
        let status_str = res["status"].as_str().unwrap_or("");
        let added = res["added"].as_i64().unwrap_or(0);
        let unconsumed: Vec<String> = res["unconsumed"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let error = res["error"].as_str().map(String::from);

        let final_status = if !ok || error.is_some() {
            "ERROR"
        } else if status_str == "waiting_input" {
            "STRANDED"
        } else if !unconsumed.is_empty() {
            "DROPPED_TOKENS"
        } else {
            "PASS"
        };

        let details = if final_status == "PASS" {
            format!("Completed successfully (added {} entities)", added)
        } else if let Some(ref e) = error {
            format!("Error: {}", e)
        } else if final_status == "STRANDED" {
            format!("Stranded waiting for more input (blocked_by: {:?})", res["blocked_by"])
        } else if final_status == "DROPPED_TOKENS" {
            format!("Unconsumed tokens left behind: {:?}", unconsumed)
        } else {
            format!("Unexpected outcome: {}", res)
        };

        // Cancel any stranded command so it doesn't pollute the next test
        if app.tabs[app.active_tab].active_cmd.is_some() {
            let _ = app.automation_op(r#"{"op":"run","cmd":"ESC"}"#);
        }

        TestRecord {
            category: cat,
            command: cmd,
            option: opt,
            cmd_input: input.to_string(),
            status: final_status,
            added_entities: added,
            unconsumed,
            error,
            details,
        }
    }

    fn add_dummy_line(app: &mut OpenCADStudio, x1: f64, y1: f64, x2: f64, y2: f64) -> Handle {
        let i = app.active_tab;
        let mut line = Line::new();
        line.start = Vector3::new(x1, y1, 0.0);
        line.end = Vector3::new(x2, y2, 0.0);
        app.tabs[i].scene.add_entity(EntityType::Line(line))
    }

    fn add_dummy_circle(app: &mut OpenCADStudio, cx: f64, cy: f64, r: f64) -> Handle {
        let i = app.active_tab;
        let mut c = Circle::new();
        c.center = Vector3::new(cx, cy, 0.0);
        c.radius = r;
        app.tabs[i].scene.add_entity(EntityType::Circle(c))
    }

    fn add_dummy_lwpoly(app: &mut OpenCADStudio, pts: &[(f64, f64)], closed: bool) -> Handle {
        let i = app.active_tab;
        let mut pl = LwPolyline::new();
        pl.is_closed = closed;
        for (x, y) in pts {
            pl.vertices.push(LwVertex::from_coords(*x, *y));
        }
        app.tabs[i].scene.add_entity(EntityType::LwPolyline(pl))
    }

    #[test]
    fn test_all_cad_commands_and_options() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let mut records = Vec::new();

        // ═════════════════════════════════════════════════════════════════════
        // 1. 2D DRAW COMMANDS
        // ═════════════════════════════════════════════════════════════════════
        // LINE
        records.push(run_test(&mut app, "Draw", "LINE", "Standard 2-point", "LINE 0,0 10,0 "));
        records.push(run_test(&mut app, "Draw", "LINE", "Close (C)", "LINE 0,0 10,0 10,10 C"));
        records.push(run_test(&mut app, "Draw", "LINE", "Undo (U)", "LINE 0,0 10,0 U 0,10 "));

        // CIRCLE
        records.push(run_test(&mut app, "Draw", "CIRCLE", "Center + Radius", "CIRCLE 0,0 5"));
        records.push(run_test(&mut app, "Draw", "CIRCLE", "Diameter (D)", "CIRCLE 0,0 D 10"));
        records.push(run_test(&mut app, "Draw", "CIRCLE", "3-Point (3P)", "CIRCLE 3P 0,0 10,0 5,5"));
        records.push(run_test(&mut app, "Draw", "CIRCLE", "2-Point (2P)", "CIRCLE 2P 0,0 10,0"));
        records.push(run_test(&mut app, "Draw", "CIRCLE", "TTR (Tan Tan Rad)", "CIRCLE TTR 0,0 10,0 5"));
        records.push(run_test(&mut app, "Draw", "CIRCLE", "TTT (Tan Tan Tan)", "CIRCLE TTT 0,0 10,0 5,5"));

        // ARC
        records.push(run_test(&mut app, "Draw", "ARC", "3-Point", "ARC 0,0 5,5 10,0"));
        records.push(run_test(&mut app, "Draw", "ARC", "Center (C)", "ARC 0,0 C 5,0 10,0"));
        records.push(run_test(&mut app, "Draw", "ARC", "Center + Angle (A)", "ARC 0,0 C 5,0 A 90"));
        records.push(run_test(&mut app, "Draw", "ARC", "Center + Chord Length (L)", "ARC 0,0 C 5,0 L 5"));
        records.push(run_test(&mut app, "Draw", "ARC", "End + Radius (R)", "ARC 0,0 E 10,0 R 5"));
        records.push(run_test(&mut app, "Draw", "ARC", "End + Angle (A)", "ARC 0,0 E 10,0 A 180"));
        records.push(run_test(&mut app, "Draw", "ARC", "End + Direction (D)", "ARC 0,0 E 10,0 D 0,10"));

        // RECTANG
        records.push(run_test(&mut app, "Draw", "RECTANG", "2 Corners", "RECTANG 0,0 10,10"));
        records.push(run_test(&mut app, "Draw", "RECTANG", "Chamfer (C)", "RECTANG C 1 1 0,0 10,10"));
        records.push(run_test(&mut app, "Draw", "RECTANG", "Fillet (F)", "RECTANG F 2 0,0 10,10"));
        records.push(run_test(&mut app, "Draw", "RECTANG", "Width (W)", "RECTANG W 0.5 0,0 10,10"));
        records.push(run_test(&mut app, "Draw", "RECTANG", "Elevation (E)", "RECTANG E 5 0,0 10,10"));
        records.push(run_test(&mut app, "Draw", "RECTANG", "Thickness (T)", "RECTANG T 2 0,0 10,10"));
        records.push(run_test(&mut app, "Draw", "RECTANG", "Dimensions (D)", "RECTANG 0,0 D 10 5 10,5"));
        records.push(run_test(&mut app, "Draw", "RECTANG", "Area (A)", "RECTANG 0,0 A 50 L 10 10,5"));
        records.push(run_test(&mut app, "Draw", "RECTANG", "Rotation (R)", "RECTANG 0,0 R 45 10,10"));

        // POLYGON
        records.push(run_test(&mut app, "Draw", "POLYGON", "Inscribed (I)", "POLYGON 6 0,0 I 10"));
        records.push(run_test(&mut app, "Draw", "POLYGON", "Circumscribed (C)", "POLYGON 6 0,0 C 10"));
        records.push(run_test(&mut app, "Draw", "POLYGON", "Edge (E)", "POLYGON 6 E 0,0 5,0"));

        // PLINE
        records.push(run_test(&mut app, "Draw", "PLINE", "Line segments + Close", "PLINE 0,0 10,0 10,10 C"));
        records.push(run_test(&mut app, "Draw", "PLINE", "Width (W)", "PLINE 0,0 W 1 2 10,0 "));
        records.push(run_test(&mut app, "Draw", "PLINE", "Halfwidth (H)", "PLINE 0,0 H 0.5 1 10,0 "));
        records.push(run_test(&mut app, "Draw", "PLINE", "Arc mode (A)", "PLINE 0,0 10,0 A 15,5 L 20,5 "));
        records.push(run_test(&mut app, "Draw", "PLINE", "Length (L)", "PLINE 0,0 10,0 L 5 "));

        // ELLIPSE
        records.push(run_test(&mut app, "Draw", "ELLIPSE", "Axis endpoints + dist", "ELLIPSE 0,0 10,0 3"));
        records.push(run_test(&mut app, "Draw", "ELLIPSE", "Center (C)", "ELLIPSE C 5,0 10,0 3"));
        records.push(run_test(&mut app, "Draw", "ELLIPSE", "Arc (A)", "ELLIPSE A 0,0 10,0 3 0 180"));

        // DONUT
        records.push(run_test(&mut app, "Draw", "DONUT", "Ring (inside, outside)", "DONUT 5 10 0,0 "));
        records.push(run_test(&mut app, "Draw", "DONUT", "Solid disc (0 inside)", "DONUT 0 10 0,0 "));

        // SPLINE
        records.push(run_test(&mut app, "Draw", "SPLINE", "Fit points", "SPLINE 0,0 5,5 10,2 15,10   "));
        records.push(run_test(&mut app, "Draw", "SPLINE", "Close (C)", "SPLINE 0,0 10,0 10,10 C "));

        // HELIX
        records.push(run_test(&mut app, "Draw", "HELIX", "Base + top + height", "HELIX 0,0 5 5 20"));
        records.push(run_test(&mut app, "Draw", "HELIX", "Turns (T)", "HELIX 0,0 5 5 T 10 20"));

        // POINT, RAY, XLINE
        records.push(run_test(&mut app, "Draw", "POINT", "Coordinate", "POINT 5,5,0"));
        records.push(run_test(&mut app, "Draw", "RAY", "Start + through", "RAY 0,0 10,10 "));
        records.push(run_test(&mut app, "Draw", "XLINE", "Horizontal (H)", "XLINE H 0,5 "));
        records.push(run_test(&mut app, "Draw", "XLINE", "Vertical (V)", "XLINE V 5,0 "));
        records.push(run_test(&mut app, "Draw", "XLINE", "Angle (A)", "XLINE A 45 0,0 "));
        records.push(run_test(&mut app, "Draw", "XLINE", "Bisect (B)", "XLINE B 0,0 10,0 0,10 "));

        // WIPEOUT & REVCLOUD
        records.push(run_test(&mut app, "Draw", "WIPEOUT", "Polygonal vertices", "WIPEOUT 0,0 10,0 10,10 0,10 C"));
        records.push(run_test(&mut app, "Draw", "WIPEOUT", "Frames (F)", "WIPEOUT F ON"));
        records.push(run_test(&mut app, "Draw", "REVCLOUD", "Arc length (A)", "REVCLOUD A 2 2 0,0 10,0 10,10 0,0"));

        // SOLID & TRACE
        records.push(run_test(&mut app, "Draw", "SOLID", "4 vertices", "SOLID 0,0 10,0 0,10 10,10 "));
        records.push(run_test(&mut app, "Draw", "TRACE", "Width + vertices", "TRACE 2 0,0 10,0 10,10  "));

        // MLINE
        records.push(run_test(&mut app, "Draw", "MLINE", "Points + Close", "MLINE 0,0 10,0 10,10 C"));
        records.push(run_test(&mut app, "Draw", "MLINE", "Scale (S)", "MLINE S 2 0,0 10,0 "));
        records.push(run_test(&mut app, "Draw", "MLINE", "Justification (J)", "MLINE J T 0,0 10,0 "));

        // HATCH & BOUNDARY
        records.push(run_test(&mut app, "Draw", "HATCH", "Pattern + scale + angle", "HATCH ANSI31 1 0 0,0 "));
        records.push(run_test(&mut app, "Draw", "BOUNDARY", "Pick point", "BOUNDARY 5,5 "));

        // ═════════════════════════════════════════════════════════════════════
        // 2. 2D MODIFY COMMANDS
        // ═════════════════════════════════════════════════════════════════════
        let h_line = add_dummy_line(&mut app, 0.0, 0.0, 10.0, 0.0);
        let h_circle = add_dummy_circle(&mut app, 20.0, 20.0, 5.0);
        let h_poly = add_dummy_lwpoly(&mut app, &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)], true);

        // MOVE
        let cmd_move = format!("MOVE {:X}  0,0 5,5", h_line);
        records.push(run_test(&mut app, "Modify", "MOVE", "Displacement", &cmd_move));

        // COPY
        let cmd_copy = format!("COPY {:X}  0,0 10,10 ", h_circle);
        records.push(run_test(&mut app, "Modify", "COPY", "Single copy", &cmd_copy));
        let cmd_copym = format!("COPY {:X}  M 0,0 5,5 10,10 ", h_circle);
        records.push(run_test(&mut app, "Modify", "COPY", "Multiple mode (M)", &cmd_copym));

        // ROTATE
        let cmd_rot = format!("ROTATE {:X}  0,0 45", h_line);
        records.push(run_test(&mut app, "Modify", "ROTATE", "Angle", &cmd_rot));
        let cmd_rot_c = format!("ROTATE {:X}  0,0 C 90", h_line);
        records.push(run_test(&mut app, "Modify", "ROTATE", "Copy (C)", &cmd_rot_c));
        let cmd_rot_r = format!("ROTATE {:X}  0,0 R 45 90", h_line);
        records.push(run_test(&mut app, "Modify", "ROTATE", "Reference (R)", &cmd_rot_r));

        // SCALE
        let cmd_sc = format!("SCALE {:X}  0,0 2", h_line);
        records.push(run_test(&mut app, "Modify", "SCALE", "Scale factor", &cmd_sc));
        let cmd_sc_c = format!("SCALE {:X}  0,0 C 1.5", h_line);
        records.push(run_test(&mut app, "Modify", "SCALE", "Copy (C)", &cmd_sc_c));
        let cmd_sc_r = format!("SCALE {:X}  0,0 R 5 10", h_line);
        records.push(run_test(&mut app, "Modify", "SCALE", "Reference (R)", &cmd_sc_r));

        // MIRROR
        let cmd_mir_n = format!("MIRROR {:X}  0,0 0,10 N", h_line);
        records.push(run_test(&mut app, "Modify", "MIRROR", "Keep source (N)", &cmd_mir_n));
        let cmd_mir_y = format!("MIRROR {:X}  0,0 0,10 Y", h_line);
        records.push(run_test(&mut app, "Modify", "MIRROR", "Delete source (Y)", &cmd_mir_y));

        // OFFSET
        let cmd_off = format!("OFFSET 2 {:X} 5,5 ", h_line);
        records.push(run_test(&mut app, "Modify", "OFFSET", "Distance", &cmd_off));
        let cmd_off_t = format!("OFFSET T {:X} 5,5 ", h_line);
        records.push(run_test(&mut app, "Modify", "OFFSET", "Through (T)", &cmd_off_t));

        // FILLET
        let l1 = add_dummy_line(&mut app, 0.0, 0.0, 10.0, 0.0);
        let l2 = add_dummy_line(&mut app, 10.0, 0.0, 10.0, 10.0);
        records.push(run_test(&mut app, "Modify", "FILLET", "Set Radius (R)", "FILLET R 2"));
        let cmd_fil = format!("FILLET {:X} {:X}", l1, l2);
        records.push(run_test(&mut app, "Modify", "FILLET", "Apply to lines", &cmd_fil));
        let cmd_fil_p = format!("FILLET P {:X}", h_poly);
        records.push(run_test(&mut app, "Modify", "FILLET", "Polyline (P)", &cmd_fil_p));

        // CHAMFER
        records.push(run_test(&mut app, "Modify", "CHAMFER", "Set Distances (D)", "CHAMFER D 2 3"));
        let cmd_ch = format!("CHAMFER {:X} {:X}", l1, l2);
        records.push(run_test(&mut app, "Modify", "CHAMFER", "Apply to lines", &cmd_ch));
        let cmd_ch_p = format!("CHAMFER P {:X}", h_poly);
        records.push(run_test(&mut app, "Modify", "CHAMFER", "Polyline (P)", &cmd_ch_p));

        // TRIM & EXTEND
        let c1 = add_dummy_line(&mut app, 5.0, -5.0, 5.0, 15.0);
        let t1 = add_dummy_line(&mut app, 0.0, 5.0, 10.0, 5.0);
        let cmd_trim = format!("TRIM {:X}  {:X} ", c1, t1);
        records.push(run_test(&mut app, "Modify", "TRIM", "Standard trim", &cmd_trim));
        let cmd_ext = format!("EXTEND {:X}  {:X} ", c1, t1);
        records.push(run_test(&mut app, "Modify", "EXTEND", "Standard extend", &cmd_ext));

        // BREAK, JOIN, EXPLODE, ERASE
        let cmd_br = format!("BREAK {:X} F 2,0 8,0", h_line);
        records.push(run_test(&mut app, "Modify", "BREAK", "First point (F)", &cmd_br));
        let cmd_jn = format!("JOIN {:X} {:X} ", l1, l2);
        records.push(run_test(&mut app, "Modify", "JOIN", "Join lines", &cmd_jn));
        let cmd_exp = format!("EXPLODE {:X} ", h_poly);
        records.push(run_test(&mut app, "Modify", "EXPLODE", "Explode polyline", &cmd_exp));
        let cmd_er = format!("ERASE {:X} ", h_line);
        records.push(run_test(&mut app, "Modify", "ERASE", "Erase entity", &cmd_er));

        // ARRAYRECT & ARRAYPOLAR
        let cmd_arr_r = format!("ARRAYRECT {:X}  3 4 10 10", h_circle);
        records.push(run_test(&mut app, "Modify", "ARRAYRECT", "Rows + columns", &cmd_arr_r));
        let cmd_arr_p = format!("ARRAYPOLAR {:X}  0,0 6 360 Y", h_circle);
        records.push(run_test(&mut app, "Modify", "ARRAYPOLAR", "Polar 6 items", &cmd_arr_p));

        // LENGTHEN & ALIGN
        let cmd_len_de = format!("LENGTHEN DE 5 {:X} ", l1);
        records.push(run_test(&mut app, "Modify", "LENGTHEN", "Delta (DE)", &cmd_len_de));
        let cmd_len_p = format!("LENGTHEN P 150 {:X} ", l1);
        records.push(run_test(&mut app, "Modify", "LENGTHEN", "Percent (P)", &cmd_len_p));
        let cmd_align = format!("ALIGN {:X}  0,0 10,10 5,0 15,10  N", l1);
        records.push(run_test(&mut app, "Modify", "ALIGN", "2D align (N)", &cmd_align));

        // PEDIT
        let cmd_ped_c = format!("PEDIT {:X} C ", h_poly);
        records.push(run_test(&mut app, "Modify", "PEDIT", "Close (C)", &cmd_ped_c));
        let cmd_ped_w = format!("PEDIT {:X} W 1.5 ", h_poly);
        records.push(run_test(&mut app, "Modify", "PEDIT", "Width (W)", &cmd_ped_w));

        // ═════════════════════════════════════════════════════════════════════
        // 3. ANNOTATION & DIMENSION COMMANDS
        // ═════════════════════════════════════════════════════════════════════
        records.push(run_test(&mut app, "Annotation", "TEXT", "Start + H + Rot + Text", "TEXT 0,0 2.5 0 HelloText"));
        records.push(run_test(&mut app, "Annotation", "TEXT", "Justify (J MC)", "TEXT J MC 5,5 2.5 0 CenterText"));
        records.push(run_test(&mut app, "Annotation", "MTEXT", "Corners + Text", "MTEXT 0,10 10,0 MultiLineText"));

        records.push(run_test(&mut app, "Dimension", "DIMLINEAR", "Linear horizontal", "DIMLINEAR 0,0 10,0 5,2"));
        records.push(run_test(&mut app, "Dimension", "DIMALIGNED", "Aligned dimension", "DIMALIGNED 0,0 10,10 5,7"));
        let cmd_dimrad = format!("DIMRADIUS {:X} 15,15", h_circle);
        records.push(run_test(&mut app, "Dimension", "DIMRADIUS", "Radius pick", &cmd_dimrad));
        let cmd_dimdia = format!("DIMDIAMETER {:X} 15,15", h_circle);
        records.push(run_test(&mut app, "Dimension", "DIMDIAMETER", "Diameter pick", &cmd_dimdia));
        let cmd_dimang = format!("DIMANGULAR {:X} {:X} 5,5", l1, l2);
        records.push(run_test(&mut app, "Dimension", "DIMANGULAR", "Angular between lines", &cmd_dimang));
        let cmd_cmark = format!("CENTERMARK {:X}", h_circle);
        records.push(run_test(&mut app, "Dimension", "CENTERMARK", "Center mark circle", &cmd_cmark));
        let cmd_cline = format!("CENTERLINE {:X} {:X}", l1, l2);
        records.push(run_test(&mut app, "Dimension", "CENTERLINE", "Centerline lines", &cmd_cline));

        // ═════════════════════════════════════════════════════════════════════
        // 4. INQUIRY, SETUP & 3D SOLIDS
        // ═════════════════════════════════════════════════════════════════════
        records.push(run_test(&mut app, "Inquiry", "DIST", "Point to point", "DIST 0,0 10,10"));
        records.push(run_test(&mut app, "Inquiry", "AREA", "Point perimeter", "AREA 0,0 10,0 10,10 0,10 "));
        records.push(run_test(&mut app, "Inquiry", "ID", "Point coordinate", "ID 10,20,30"));

        records.push(run_test(&mut app, "Setup", "LAYER", "Make + Color", "LAYER M MyLayer C Red MyLayer "));
        records.push(run_test(&mut app, "Setup", "COLOR", "Set color name", "COLOR Red"));
        records.push(run_test(&mut app, "Setup", "COLOR", "Set color index", "COLOR 1"));
        records.push(run_test(&mut app, "Setup", "LINETYPE", "Set linetype", "LINETYPE S HIDDEN "));
        records.push(run_test(&mut app, "Setup", "ZOOM", "Extents (E)", "ZOOM E"));
        records.push(run_test(&mut app, "Setup", "ZOOM", "Window (W)", "ZOOM W 0,0 100,100"));

        records.push(run_test(&mut app, "Solid3D", "BOX", "Corners + height", "BOX 0,0,0 10,10,10"));
        records.push(run_test(&mut app, "Solid3D", "CYLINDER", "Center + rad + height", "CYLINDER 0,0,0 5 10"));
        records.push(run_test(&mut app, "Solid3D", "SPHERE", "Center + radius", "SPHERE 0,0,0 5"));
        records.push(run_test(&mut app, "Solid3D", "CONE", "Center + rad + height", "CONE 0,0,0 5 10"));
        records.push(run_test(&mut app, "Solid3D", "PYRAMID", "Center + rad + height", "PYRAMID 0,0,0 5 10"));
        records.push(run_test(&mut app, "Solid3D", "WEDGE", "Corners + height", "WEDGE 0,0,0 10,10,10"));
        records.push(run_test(&mut app, "Solid3D", "TORUS", "Center + r1 + r2", "TORUS 0,0,0 10 2"));

        // ═════════════════════════════════════════════════════════════════════
        // PRINT CONSOLIDATED REPORT
        // ═════════════════════════════════════════════════════════════════════
        println!("\n=================== CAD COMMANDS & OPTIONS GAP REPORT ===================");
        let mut pass = 0;
        let mut dropped = 0;
        let mut stranded = 0;
        let mut err = 0;
        for r in &records {
            println!(
                "[{}] {} | {} ({}) -> {}",
                r.status, r.category, r.command, r.option, r.details
            );
            match r.status {
                "PASS" => pass += 1,
                "DROPPED_TOKENS" => dropped += 1,
                "STRANDED" => stranded += 1,
                _ => err += 1,
            }
        }
        println!("-------------------------------------------------------------------------");
        println!(
            "TOTAL: {} | PASS: {} | DROPPED_TOKENS: {} | STRANDED: {} | ERROR: {}",
            records.len(),
            pass,
            dropped,
            stranded,
            err
        );
        println!("=========================================================================\n");

        assert!(pass > 0, "At least some CAD commands must pass");
    }
}
