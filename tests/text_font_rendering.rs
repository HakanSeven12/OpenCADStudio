use acadrust::entities::{Insert, MText};
use acadrust::tables::{BlockRecord, TextStyle};
use acadrust::types::Vector3;
use acadrust::{CadDocument, EntityType, Handle};
use OpenCADStudio::scene::cache::block_cache::{expand_insert, BlockCache};
use OpenCADStudio::scene::view::render::InheritStyle;
use OpenCADStudio::scene::WireModel;

fn drawable_point_count(wires: &[WireModel]) -> usize {
    wires
        .iter()
        .map(|w| w.points.iter().filter(|p| p[0].is_finite()).count() + w.fill_tris.len())
        .sum()
}

/// Build a one-block document holding a single MTEXT and expand the INSERT
/// through the block cache, returning the finalized wires. `mtext_at` is the
/// MTEXT's position inside the block; `insert_at` is where the block is placed.
fn expand_block_mtext(
    value: &str,
    font_file: &str,
    mtext_at: Vector3,
    insert_at: Vector3,
) -> Vec<WireModel> {
    let mut doc = CadDocument::new();

    let mut style = TextStyle::new("SHOP");
    style.font_file = font_file.to_string();
    doc.text_styles.add(style).unwrap();

    let br_h = Handle::new(doc.next_handle());
    let mut br = BlockRecord::new("LABEL_BLOCK");
    br.handle = br_h;
    doc.block_records.add(br).unwrap();

    let mut mtext = MText::with_value(value, mtext_at);
    mtext.style = "SHOP".to_string();
    mtext.height = 20.0;
    mtext.rectangle_width = 0.0;
    let mut sub = EntityType::MText(mtext);
    sub.common_mut().owner_handle = br_h;
    doc.add_entity(sub).unwrap();

    let ins = Insert::new("LABEL_BLOCK", insert_at);
    doc.add_entity(EntityType::Insert(ins.clone())).unwrap();
    let cache = BlockCache::build(&doc, 1.0, [0.0, 0.0, 0.0, 1.0]);
    expand_insert(
        &cache,
        &ins,
        Handle::new(999),
        [1.0, 1.0, 1.0, 1.0],
        0.0,
        [0.0; 8],
        1.0,
        // The INSERT sits on layer "0" which resolves to the white/Continuous
        // fallback — matching the resolved style passed above.
        InheritStyle {
            color: [1.0, 1.0, 1.0, 1.0],
            pat_len: 0.0,
            pat: [0.0; 8],
            lw_px: 1.0,
        },
        false,
        1.0,
        None,
        None,
        false,
        [0.0, 0.0, 0.0, 1.0],
    )
    .expect("block defn is cached")
}

#[test]
fn block_nested_mtext_uses_its_style_font() {
    // NB: on a host without Arial, `arial.ttf` resolves to the LFF stroke
    // fallback, so this exercises the "something renders" path rather than the
    // TTF canonicalisation specifically. The font-canonicalisation logic itself
    // is covered by the unit tests in `text_support`; the colour-split and
    // fill-pairing tests below cover the block-cache rendering changes.
    let wires = expand_block_mtext(
        "FERRAGAMO",
        "arial.ttf",
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(100.0, 50.0, 0.0),
    );

    assert!(
        drawable_point_count(&wires) > 0,
        "block-nested MTEXT should render through its text style font"
    );

    assert!(
        wires
            .iter()
            .all(|w| w.points.is_empty() || w.fill_tris.is_empty()),
        "outline and fill wires should be separate for correct GPU classification"
    );
}

#[test]
fn block_nested_colour_split_mtext_keeps_per_wire_colour() {
    // `\C1;` = ACI red, `\C2;` = ACI yellow → two colour bins. The block cache
    // must keep them as separate per-wire colours; the fold-to-one-colour bug
    // this PR fixes silently collapsed every segment to the first colour. Uses
    // the builtin stroke font so the split is observable without a system TTF.
    let wires = expand_block_mtext(
        "\\C1;AAA\\C2;BBB",
        "txt",
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(100.0, 50.0, 0.0),
    );

    let mut colours: Vec<[u8; 3]> = wires
        .iter()
        .filter(|w| !w.points.is_empty() || !w.fill_tris.is_empty())
        .map(|w| {
            [
                (w.color[0] * 255.0).round() as u8,
                (w.color[1] * 255.0).round() as u8,
                (w.color[2] * 255.0).round() as u8,
            ]
        })
        .collect();
    colours.sort();
    colours.dedup();

    assert!(
        colours.len() >= 2,
        "colour-split MTEXT in a block must keep ≥2 distinct wire colours, got {colours:?}"
    );
}

#[test]
fn block_nested_mtext_fill_tris_keep_paired_low_half_at_utm() {
    // TTF glyph fills are host-dependent (builtin LFF stroke fonts produce
    // none). When the host resolves a TTF and fills ARE produced, every fill
    // wire must carry an index-paired `fill_tris_low`: emit_wire reconstructs
    // `fill_tris[i] + fill_tris_low[i]`, so an unpaired wire panics (release
    // too) and a dropped low half quantizes fills to ~0.5 m at UTM scale.
    // The MTEXT sits at a UTM-scale coordinate so the low half is significant.
    // "DejaVu Sans" is present on most Linux hosts; where no TTF resolves the
    // fill set is empty and the assertions are skipped (gate below).
    let wires = expand_block_mtext(
        "FERRAGAMO",
        "DejaVu Sans",
        Vector3::new(500_000.0, 4_000_000.0, 0.0),
        Vector3::new(0.0, 0.0, 0.0),
    );

    let fill_wires: Vec<&WireModel> = wires.iter().filter(|w| !w.fill_tris.is_empty()).collect();
    if fill_wires.is_empty() {
        eprintln!("no TTF fills resolvable on this host; skipping fill-pairing assertions");
        return;
    }

    for w in &fill_wires {
        assert_eq!(
            w.fill_tris.len(),
            w.fill_tris_low.len(),
            "fill_tris and fill_tris_low must be index-paired for emit_wire"
        );
    }

    let any_nonzero_low = fill_wires
        .iter()
        .flat_map(|w| w.fill_tris_low.iter())
        .any(|p| p.iter().any(|&c| c != 0.0));
    assert!(
        any_nonzero_low,
        "UTM-scale fills must keep a non-zero low half — a dropped fill_tris_low \
         silently quantizes fill triangles to ~0.5 m"
    );
}
