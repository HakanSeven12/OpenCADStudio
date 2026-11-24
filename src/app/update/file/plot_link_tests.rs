//! Links between the layouts of one drawing written into one PDF.

use super::plot::link_layouts_within;
use crate::io::pdf_export::PdfPageInput;
use crate::io::plot::{PlotContent, PlotLink};

fn page(links: Vec<PlotLink>) -> PdfPageInput {
    PdfPageInput {
        content: PlotContent {
            links,
            ..Default::default()
        },
        paper_w: 420.0,
        paper_h: 297.0,
        offset_x: 0.0,
        offset_y: 0.0,
        rotation_deg: 0,
        content_rotation: 0.0,
        scale: 1.0,
        clip: None,
        options: Default::default(),
        plot_style: None,
        geopdf: None,
    }
}

fn link(url: &str, location: &str) -> PlotLink {
    PlotLink {
        url: url.into(),
        location: location.into(),
        ..Default::default()
    }
}

/// A link to a layout written into the same file opens its page, by the
/// layout's name in any case, also as a `view,layout` location; a link to
/// another drawing or a layout not written stays an address. Only pages
/// links reach get a name.
#[test]
fn a_link_to_another_layout_of_the_drawing_opens_its_page() {
    let mut pages = vec![
        page(vec![
            link("", "plan b"),
            link("", "Front,PLAN C"),
            link("other.dwg", "Plan B"),
            link("", "Plan D"),
        ]),
        page(Vec::new()),
        page(Vec::new()),
    ];
    let names = ["Plan A", "Plan B", "Plan C"].map(String::from);
    link_layouts_within(&mut pages, &names, None);
    let destinations: Vec<Option<&str>> = pages[0]
        .content
        .links
        .iter()
        .map(|link| link.destination.as_deref())
        .collect();
    assert_eq!(
        destinations,
        vec![
            Some("OcsLayout Plan B"),
            Some("OcsLayout Plan C"),
            None,
            None
        ]
    );
    let named: Vec<Option<&str>> = pages
        .iter()
        .map(|page| page.content.destination.as_deref())
        .collect();
    assert_eq!(
        named,
        vec![None, Some("OcsLayout Plan B"), Some("OcsLayout Plan C")]
    );
}

/// The drawing's own path, however written, is this drawing.
#[test]
fn a_link_to_the_drawing_itself_by_its_path_stays_inside() {
    let dir = std::env::temp_dir().join(format!("ocs-own-link-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let own = dir.join("a.dwg");
    std::fs::write(&own, b"").unwrap();
    let mut by_path = link("../a.dwg", "Plan B");
    by_path.base = Some(dir.join("sub"));
    let mut pages = vec![page(vec![by_path]), page(Vec::new())];
    link_layouts_within(&mut pages, &["Plan A".into(), "Plan B".into()], Some(&own));
    assert_eq!(
        pages[0].content.links[0].destination.as_deref(),
        Some("OcsLayout Plan B")
    );
    let _ = std::fs::remove_dir_all(&dir);
}
