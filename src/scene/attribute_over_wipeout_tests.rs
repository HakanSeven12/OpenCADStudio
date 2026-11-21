//! An insert's attributes are drawn over the block's own members: a panel
//! block holding a wipeout over the panel keeps its attribute number visible
//! while the insert is neither hovered nor selected (the xray pass that shows
//! it then ignores depth).

use crate::scene::Scene;

type Pairs = Vec<(i32, String)>;

fn push(pairs: &mut Pairs, items: &[(i32, &str)]) {
    pairs.extend(items.iter().map(|(code, value)| (*code, value.to_string())));
}

fn wipeout(pairs: &mut Pairs, id: &str) {
    push(
        pairs,
        &[
            (0, "WIPEOUT"),
            (5, id),
            (100, "AcDbEntity"),
            (8, "0"),
            (100, "AcDbWipeout"),
            (90, "0"),
            (10, "0"),
            (20, "0"),
            (30, "0"),
            (11, "8"),
            (21, "0"),
            (31, "0"),
            (12, "0"),
            (22, "5"),
            (32, "0"),
            (13, "1"),
            (23, "1"),
            (70, "7"),
            (280, "1"),
            (281, "50"),
            (282, "50"),
            (283, "0"),
            (71, "1"),
            (91, "2"),
            (14, "-0.5"),
            (24, "-0.5"),
            (14, "0.5"),
            (24, "0.5"),
        ],
    );
}

/// An ATTDEF (`attrib` false) or ATTRIB of the tag NUM, centred at `(x, y)`.
fn num_text(pairs: &mut Pairs, attrib: bool, id: &str, value: &str, x: i32, y: f64) {
    let (x, y) = (x.to_string(), y.to_string());
    push(
        pairs,
        &[
            (0, if attrib { "ATTRIB" } else { "ATTDEF" }),
            (5, id),
            (100, "AcDbEntity"),
            (8, "0"),
            (100, "AcDbText"),
            (10, &x),
            (20, &y),
            (30, "0"),
            (40, "0.25"),
            (1, value),
            (72, "1"),
            (11, &x),
            (21, &y),
            (31, "0"),
            (
                100,
                if attrib {
                    "AcDbAttribute"
                } else {
                    "AcDbAttributeDefinition"
                },
            ),
            (280, "0"),
        ],
    );
    if !attrib {
        push(pairs, &[(3, "NUM")]);
    }
    push(pairs, &[(2, "NUM"), (70, "0"), (74, "2")]);
}

/// A drawing of one block `PANEL` (a WIPEOUT over an 8 x 5 panel, an outline
/// and the attribute definition NUM) inserted four times with the NUM values
/// 1 to 4. The wipeout is the block's first member, or its last one.
fn panels(wipeout_first: bool) -> String {
    let mut handle = 0x100u32;
    let mut next = move || {
        handle += 1;
        format!("{handle:X}")
    };
    let mut p = Pairs::new();
    push(
        &mut p,
        &[
            (0, "SECTION"),
            (2, "CLASSES"),
            (0, "CLASS"),
            (1, "WIPEOUT"),
            (2, "AcDbWipeout"),
            (3, "WipeOut"),
            (90, "127"),
            (91, "0"),
            (280, "0"),
            (281, "1"),
            (0, "ENDSEC"),
            (0, "SECTION"),
            (2, "BLOCKS"),
            (0, "BLOCK"),
            (8, "0"),
            (2, "PANEL"),
            (70, "2"),
            (10, "0"),
            (20, "0"),
            (30, "0"),
            (3, "PANEL"),
        ],
    );
    if wipeout_first {
        wipeout(&mut p, &next());
    }
    push(
        &mut p,
        &[
            (0, "LWPOLYLINE"),
            (5, &next()),
            (100, "AcDbEntity"),
            (8, "0"),
            (100, "AcDbPolyline"),
            (90, "4"),
            (70, "1"),
            (10, "0"),
            (20, "0"),
            (10, "8"),
            (20, "0"),
            (10, "8"),
            (20, "5"),
            (10, "0"),
            (20, "5"),
        ],
    );
    num_text(&mut p, false, &next(), "0", 4, 2.5);
    if !wipeout_first {
        wipeout(&mut p, &next());
    }
    push(
        &mut p,
        &[
            (0, "ENDBLK"),
            (8, "0"),
            (0, "ENDSEC"),
            (0, "SECTION"),
            (2, "ENTITIES"),
        ],
    );
    for i in 0..4 {
        let (x, y) = (10 * (i % 2), 7 * (i / 2));
        push(
            &mut p,
            &[
                (0, "INSERT"),
                (5, &next()),
                (100, "AcDbEntity"),
                (8, "0"),
                (100, "AcDbBlockReference"),
                (66, "1"),
                (2, "PANEL"),
                (10, &x.to_string()),
                (20, &y.to_string()),
                (30, "0"),
            ],
        );
        num_text(
            &mut p,
            true,
            &next(),
            &(i + 1).to_string(),
            4 + x,
            2.5 + f64::from(y),
        );
        push(&mut p, &[(0, "SEQEND"), (8, "0")]);
    }
    push(&mut p, &[(0, "ENDSEC"), (0, "EOF")]);
    p.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect()
}

fn load(wipeout_first: bool) -> Scene {
    let doc =
        document::DxfReader::from_reader(std::io::Cursor::new(panels(wipeout_first).into_bytes()))
            .expect("the panel drawing reads")
            .read()
            .expect("the panel drawing parses");
    let mut scene = Scene::new();
    scene.document = doc;
    scene
}

/// Every insert's attribute text is nearer to the camera than the block's
/// wipeout placed with that insert: the final depths the draw uses.
fn assert_attributes_over_wipeouts(wipeout_first: bool) {
    let scene = load(wipeout_first);
    let depths = scene.draw_depth_map();
    let wires = scene.entity_wires();
    let wipeouts = scene.wipeout_models_arc();
    assert_eq!(wipeouts.len(), 4, "one wipeout per insert");
    let mut checked = 0;
    for wire in wires.iter().filter(|wire| !wire.text_verts.is_empty()) {
        let handle: u64 = wire.name.parse().expect("named after its insert");
        let [base, half] = depths[&handle];
        let text = crate::scene::pipeline::wire_gpu::wire_draw_depth(wire, depths.as_ref());
        let own: Vec<_> = wipeouts
            .iter()
            .filter(|mask| mask.draw_depth > base && mask.draw_depth < base + 2.0 * half)
            .collect();
        assert_eq!(own.len(), 1, "the insert's own wipeout");
        eprintln!(
            "insert {handle:X}: text depth {text:.9}, wipeout depth {:.9}",
            own[0].draw_depth
        );
        assert!(
            text > own[0].draw_depth,
            "the attribute ({text}) is hidden under the block's wipeout ({})",
            own[0].draw_depth
        );
        assert!(
            text < base + 2.0 * half,
            "the attribute stays under the next insert"
        );
        checked += 1;
    }
    assert_eq!(checked, 4, "one attribute text per insert");
}

#[test]
fn an_attribute_is_drawn_over_a_wipeout_that_is_the_first_block_member() {
    assert_attributes_over_wipeouts(true);
}

#[test]
fn an_attribute_is_drawn_over_a_wipeout_that_is_the_last_block_member() {
    assert_attributes_over_wipeouts(false);
}
