//! End-to-end ribbon view construction and layout benchmark.
//!
//! Unlike the broader performance bench's widget-construction-only metric, this
//! carries Iced's widget-tree cache across frames and runs layout at a fixed size.

use std::hint::black_box;
use std::time::Instant;

use OpenCADStudio::ui::ribbon::{LayerInfo, Ribbon};
use iced::{Color, Size};
use iced_test::runtime::user_interface::{Cache, UserInterface};

fn main() {
    let mut ribbon = Ribbon::new();
    ribbon.set_styles(
        vec!["Standard".into(), "Title".into(), "Annotative".into()],
        "Standard",
        vec!["Standard".into()],
        "Standard",
        vec!["Standard".into()],
        "Standard",
        vec!["Standard".into()],
        "Standard",
    );
    ribbon.set_layers(
        ["0", "DRAWING", "DIMENSIONS", "ANNOTATIONS"]
            .into_iter()
            .map(|name| LayerInfo {
                name: name.into(),
                color: Color::TRANSPARENT,
                visible: true,
                frozen: false,
                locked: false,
                vp_frozen: None,
            })
            .collect(),
        "0",
    );

    let mut renderer = iced_test::futures::futures::executor::block_on(
        <iced::Renderer as iced_test::core::renderer::Headless>::new(
            iced_test::core::renderer::Settings::default(),
            None,
        ),
    )
    .expect("headless renderer");
    let mut cache = Cache::default();
    let bounds = Size::new(1600.0, 180.0);

    let frame = |cache, renderer: &mut iced::Renderer| {
        let ui = UserInterface::build(
            ribbon.view(None, false, false, false, 0, 0, false, None),
            bounds,
            cache,
            renderer,
        );
        black_box(ui.into_cache())
    };

    for _ in 0..10 {
        cache = frame(cache, &mut renderer);
    }

    let iterations = 100;
    let runs = 7;
    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        for _ in 0..iterations {
            cache = frame(cache, &mut renderer);
        }
        samples.push(start.elapsed().as_secs_f64() * 1_000_000.0 / iterations as f64);
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "ribbon view + Iced layout (cached steady state): median {:.1} µs/frame; samples {:?}",
        samples[runs / 2],
        samples
    );
}
