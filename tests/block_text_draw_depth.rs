// Regression: text INSIDE a block must reach the GPU at neutral vertex depth
// with its composed child rank carried in `WireModel::depth_override`, so the
// upload resolves it against the insert's scene-graph entry
// (depths[insert] + override * half) — the same composition the wipeout/hatch
// fills get. Baking the raw per-block rank into the vertices instead sinks
// text with a negative rank below its own block's wipes (the "G-3" mark
// labels of BS33-11) and catapults text with a positive rank above later
// draw-order geometry.
//
// The scene mirrors the real-world shape of the bug: a block whose children
// are [wipeout, line, text], inserted into a drawing big enough that the
// insert's child band is as fine-grained as a real one.
use acadrust::entities::{Insert, Text, Wipeout};
use acadrust::tables::BlockRecord;
use acadrust::types::Vector3;
use acadrust::EntityType;
use OpenCADStudio::scene::cache::block_cache::{expand_insert, BlockCache};
use OpenCADStudio::scene::view::render::InheritStyle;
use OpenCADStudio::scene::Scene;
use OpenCADStudio::scene::WireModel;

const DRAW_ORDER_BIAS: f32 = 0.001;
const DEPTH_QUANTA: f32 = 16_777_216.0; // 2^24 (Depth24Plus)

#[test]
fn block_text_depth_composes_through_the_instance_path() {
    let mut scene = Scene::new();

    let br_h = acadrust::Handle::new(scene.document.next_handle());
    let mut br = BlockRecord::new("MARK");
    br.handle = br_h;
    scene.document.block_records.add(br).unwrap();

    // Child 0: the wipeout the text is drawn after (the text must win).
    let mut wipe = Wipeout::new();
    wipe.insertion_point = Vector3::new(-10.0, -10.0, 0.0);
    wipe.u_vector = Vector3::new(20.0, 0.0, 0.0);
    wipe.v_vector = Vector3::new(0.0, 20.0, 0.0);
    let mut wipe_e = EntityType::Wipeout(wipe);
    wipe_e.common_mut().owner_handle = br_h;
    let wipe_handle = scene.document.add_entity(wipe_e).unwrap();

    // Child 1: a spacer so the text sits two sibling steps above the wipe,
    // the same gap as the real BS33-11 mark blocks.
    let mut spacer = acadrust::entities::Line::default();
    spacer.start = Vector3::new(-10.0, -10.0, 0.0);
    spacer.end = Vector3::new(10.0, 10.0, 0.0);
    let mut spacer_e = EntityType::Line(spacer);
    spacer_e.common_mut().owner_handle = br_h;
    let _ = scene.document.add_entity(spacer_e).unwrap();

    // Child 2: the text.
    let mut txt = Text::new();
    txt.value = "G-3".to_string();
    txt.height = 2.5;
    txt.insertion_point = Vector3::new(0.0, 0.0, 0.0);
    let mut txt_e = EntityType::Text(txt);
    txt_e.common_mut().owner_handle = br_h;
    let text_handle = scene.document.add_entity(txt_e).unwrap();

    // A large model space, so the insert's child band compresses to the
    // fine granularity real drawings have (~600 top-level entities puts the
    // per-sibling spacing at a handful of 24-bit quanta, where an oversized
    // wipeout depth bias would swallow it).
    for i in 0..600 {
        let mut line = acadrust::entities::Line::default();
        line.start = Vector3::new(f64::from(i) * 10.0 - 3000.0, -50.0, 0.0);
        line.end = Vector3::new(f64::from(i) * 10.0 - 3000.0, 50.0, 0.0);
        let _ = scene.document.add_entity(EntityType::Line(line));
    }

    let ins = Insert::new("MARK", Vector3::new(0.0, 0.0, 0.0));
    let ins_handle = scene.add_entity(EntityType::Insert(ins));

    let depths = scene.draw_depth_map();
    let [ins_d, ins_half] = depths
        .get(&ins_handle.value())
        .copied()
        .expect("insert must be ranked");
    let text_label = depths
        .get(&text_handle.value())
        .expect("block children must be ranked")[0];
    let wipe_label = depths.get(&wipe_handle.value()).expect("wipe ranked")[0];
    assert!(
        text_label > wipe_label,
        "precondition: the drawing orders the text after the wipeout"
    );

    let cache = BlockCache::build(
        &scene.document, 1.0, None, true, [0.0, 0.0, 0.0, 1.0], None, &depths,
    );
    let wires = expand_insert(
        &scene.document, &cache, &Insert::new("MARK", Vector3::new(0.0, 0.0, 0.0)),
        ins_handle,
        [1.0, 1.0, 1.0, 1.0], 0, 0.0, [0.0; 8], 1.0,
        InheritStyle { color: [1.0, 1.0, 1.0, 1.0], pat_len: 0.0, pat: [0.0; 8], lw_px: 1.0 },
        0, false, false, 1.0, None, None, false, [0.0, 0.0, 0.0, 1.0], 1.0,
        OpenCADStudio::scene::BlockScalePolicy::FromInsert, false,
    )
    .expect("block defn cached");

    let text_wires: Vec<&WireModel> =
        wires.iter().filter(|w| !w.text_verts.is_empty()).collect();
    assert!(
        !text_wires.is_empty(),
        "block TEXT must tessellate into glyph quads"
    );

    // The wire is named after its insert (decimal handle), and the upload
    // resolves that name against the depth map: this is the composition the
    // GPU finally sees.
    for w in &text_wires {
        assert!(
            w.text_verts.iter().all(|v| v.draw_depth == 0.0),
            "glyph vertices must stay at neutral depth; the instance path owns it"
        );
        assert_eq!(
            w.depth_override,
            Some(text_label),
            "the wire must carry the text's child rank as its depth override"
        );
        let resolved = w
            .name
            .parse::<u64>()
            .ok()
            .and_then(|h| depths.get(&h).copied());
        let instance = match (resolved, w.depth_override) {
            (Some([d, half]), Some(local)) => d + local * half,
            (Some([d, _]), None) => d,
            _ => 0.0,
        };
        assert_eq!(
            instance,
            ins_d + text_label * ins_half,
            "instance depth must compose to depths[insert] + label * half"
        );

        // The wipeout mask, composed the same way the scene graph does it.
        let mask = ins_d + wipe_label * ins_half;
        let margin_quanta =
            (instance - mask) * DRAW_ORDER_BIAS * DEPTH_QUANTA;
        let bias = OpenCADStudio::scene::pipeline::WIPEOUT_DEPTH_BIAS_QUANTA as f32;
        assert!(
            margin_quanta > bias + 2.0,
            "text drawn after its block's wipeout must clear it by more than \
             the wipeout depth bias plus headroom, got {margin_quanta} quanta"
        );
    }
}
