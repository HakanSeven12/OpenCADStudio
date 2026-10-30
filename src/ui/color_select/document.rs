//! The colours a drawing uses, for the Document tab.
//!
//! One pass over every object (block definitions included) and every layer
//! when the panel opens. Keys borrow from the drawing, so the pass allocates
//! only per distinct colour, never per object.

use super::math;
use codec::types::{Color as AcadColor, Handle};
use rustc_hash::FxHashMap;

/// One distinct colour: a concrete colour plus, for a colour-book colour,
/// its `BOOK$NAME` identity.
#[derive(Debug, Clone, PartialEq)]
pub struct DocColor {
    pub color: AcadColor,
    pub name: Option<String>,
    /// Objects and layers using it. Only used for sorting, never shown.
    uses: u32,
    /// The newest object or layer using it.
    newest: Handle,
}

impl DocColor {
    /// The name in the list view: the colour-book colour's own name (without
    /// the book), `ACI n` or `#RRGGBB`.
    pub fn label(&self) -> String {
        match &self.name {
            Some(name) => name
                .split_once('$')
                .map_or(name.as_str(), |(_, name)| name)
                .to_string(),
            None => math::short_name(self.color),
        }
    }
}

/// Swatches or a list of named bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DocView {
    #[default]
    Grid,
    List,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DocSort {
    #[default]
    Recent,
    Usage,
    Hue,
    Number,
    NameAsc,
    NameDesc,
}

impl DocSort {
    pub const ALL: [DocSort; 6] = [
        DocSort::Recent,
        DocSort::Usage,
        DocSort::Hue,
        DocSort::Number,
        DocSort::NameAsc,
        DocSort::NameDesc,
    ];
}

impl std::fmt::Display for DocSort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            DocSort::Recent => crate::t!("Recently used"),
            DocSort::Usage => crate::t!("Most used"),
            DocSort::Hue => crate::t!("Color"),
            DocSort::Number => crate::t!("Color number"),
            DocSort::NameAsc => crate::t!("Name A–Z"),
            DocSort::NameDesc => crate::t!("Name Z–A"),
        })
    }
}

/// A colour as a hash key: index colours keep their number, true colours
/// sit above 255.
fn key(color: AcadColor) -> Option<u32> {
    match color {
        AcadColor::Index(i) => Some(u32::from(i)),
        AcadColor::Rgb { r, g, b } => {
            Some(1 << 24 | u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b))
        }
        _ => None,
    }
}

/// A layer's colour-book identity, `BOOK$NAME`. Layers keep the book and
/// the colour name apart; objects store them joined like this.
pub fn layer_identity(layer: &codec::tables::Layer) -> Option<String> {
    match (&layer.book_name, &layer.color_name) {
        (Some(book), Some(name)) => Some(format!("{book}${name}")),
        (None, Some(name)) => Some(name.clone()),
        _ => None,
    }
}

/// Every concrete colour in the drawing. ByLayer, ByBlock and None are
/// states rather than colours and are skipped.
pub fn collect(document: &codec::Document) -> Vec<DocColor> {
    let layer_names: Vec<Option<String>> = document.layers.iter().map(layer_identity).collect();
    let objects = document.entities().map(|entity| {
        let common = entity.common();
        (common.color, common.color_name.as_deref(), common.handle)
    });
    let layers = document
        .layers
        .iter()
        .zip(&layer_names)
        .map(|(layer, name)| (layer.color, name.as_deref(), layer.handle));
    tally(objects.chain(layers))
}

fn tally<'a>(uses: impl Iterator<Item = (AcadColor, Option<&'a str>, Handle)>) -> Vec<DocColor> {
    let mut index: FxHashMap<(u32, Option<&str>), usize> = FxHashMap::default();
    let mut colors: Vec<DocColor> = Vec::new();
    for (color, name, handle) in uses {
        let Some(k) = key(color) else { continue };
        let name = name.filter(|name| !name.is_empty());
        let slot = *index.entry((k, name)).or_insert_with(|| {
            colors.push(DocColor {
                color,
                name: name.map(str::to_string),
                uses: 0,
                newest: handle,
            });
            colors.len() - 1
        });
        let entry = &mut colors[slot];
        entry.uses += 1;
        entry.newest = entry.newest.max(handle);
    }
    colors
}

/// Sort for display. `recent` is this drawing's pick history, newest first;
/// colours not picked this session follow, newest object first.
pub fn sort(colors: &mut [DocColor], by: DocSort, recent: &[AcadColor]) {
    match by {
        // The rank is looked up once per colour, not on every comparison.
        DocSort::Recent => colors.sort_by_cached_key(|c| {
            let rank = recent
                .iter()
                .position(|&r| r == c.color)
                .unwrap_or(usize::MAX);
            (rank, std::cmp::Reverse(c.newest))
        }),
        DocSort::Usage => colors.sort_by(|a, b| b.uses.cmp(&a.uses).then(b.newest.cmp(&a.newest))),
        DocSort::Hue => colors.sort_by_cached_key(|c| {
            let (r, g, b) = math::color_rgb(c.color).unwrap_or_default();
            let hsb = math::rgb_to_hsb(r, g, b);
            // Grays after the colours; within a hue band, light to dark.
            let gray = hsb.s < 0.12 || hsb.b < 0.08;
            let band = if gray {
                0
            } else {
                (hsb.h / 15.0).round() as u32
            };
            (gray, band, std::cmp::Reverse((hsb.b * 1000.0) as u32))
        }),
        DocSort::Number => colors.sort_by(|a, b| {
            let kind = |c: &DocColor| match (&c.name, c.color) {
                (None, AcadColor::Index(_)) => 0,
                (None, _) => 1,
                _ => 2,
            };
            kind(a)
                .cmp(&kind(b))
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| key(a.color).cmp(&key(b.color)))
        }),
        DocSort::NameAsc => colors.sort_by_cached_key(|c| c.label().to_lowercase()),
        DocSort::NameDesc => {
            colors.sort_by_cached_key(|c| std::cmp::Reverse(c.label().to_lowercase()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: AcadColor = AcadColor::Index(1);
    const BLUE: AcadColor = AcadColor::Index(5);
    const SKY: AcadColor = AcadColor::Rgb {
        r: 57,
        g: 191,
        b: 252,
    };

    fn sample() -> Vec<DocColor> {
        let h = Handle::new;
        tally(
            [
                (RED, None, h(1)),
                (RED, None, h(2)),
                (RED, None, h(3)),
                (AcadColor::ByLayer, None, h(4)),
                (SKY, None, h(5)),
                (SKY, Some("RAL CLASSIC$RAL 5015"), h(6)),
                (BLUE, Some(""), h(7)),
            ]
            .into_iter(),
        )
    }

    #[test]
    fn collects_each_concrete_colour_once() {
        let colors = sample();
        assert_eq!(colors.len(), 4, "ByLayer skipped, book colour kept apart");
        assert_eq!(colors[0].uses, 3);
        assert_eq!(colors[0].newest, Handle::new(3));
        assert_eq!(colors[3].name, None, "an empty name is no name");
        assert_eq!(colors[2].label(), "RAL 5015");
        assert_eq!(colors[0].label(), "ACI 1");
        assert_eq!(colors[1].label(), "#39BFFC");
    }

    #[test]
    fn sorts_four_ways() {
        let order = |by, recent: &[AcadColor]| {
            let mut colors = sample();
            sort(&mut colors, by, recent);
            colors
                .iter()
                .map(|c| (c.color, c.name.is_some()))
                .collect::<Vec<_>>()
        };
        // Picked this session first, then newest object first.
        assert_eq!(
            order(DocSort::Recent, &[RED]),
            [(RED, false), (BLUE, false), (SKY, true), (SKY, false)]
        );
        assert_eq!(order(DocSort::Usage, &[])[0], (RED, false));
        assert_eq!(
            order(DocSort::Number, &[]),
            [(RED, false), (BLUE, false), (SKY, false), (SKY, true)]
        );
        // Red (hue 0) before sky blue (hue ~199) before blue (hue 240).
        assert_eq!(order(DocSort::Hue, &[])[0], (RED, false));
        assert_eq!(order(DocSort::Hue, &[])[3], (BLUE, false));
    }
}
