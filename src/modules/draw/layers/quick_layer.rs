use crate::command::{CadCommand, CmdResult, CommandSuggestion, InputKind};
use codec::Handle;
use glam::DVec3;

/// Fast searchable layer picker.
///
/// If objects were selected when QLAYER started, the chosen layer is applied
/// to those objects. With no selection, the chosen layer becomes current.
#[derive(Clone, Debug)]
pub struct QuickLayerEntry {
    name: String,
    color: Option<codec::types::Color>,
    visible: bool,
    frozen: bool,
    locked: bool,
}

impl QuickLayerEntry {
    pub fn new(
        name: String,
        color: codec::types::Color,
        visible: bool,
        frozen: bool,
        locked: bool,
    ) -> Self {
        Self {
            name,
            color: Some(color),
            visible,
            frozen,
            locked,
        }
    }

    fn plain(name: String) -> Self {
        Self {
            name,
            color: None,
            visible: true,
            frozen: false,
            locked: false,
        }
    }
}

pub struct QuickLayerCommand {
    layers: Vec<QuickLayerEntry>,
    selected: Vec<Handle>,
}

impl QuickLayerCommand {
    pub fn new(layers: Vec<String>, selected: Vec<Handle>) -> Self {
        let entries = layers
            .into_iter()
            .map(QuickLayerEntry::plain)
            .collect();

        Self::new_with_entries(entries, selected)
    }

    pub fn new_with_entries(
        mut layers: Vec<QuickLayerEntry>,
        selected: Vec<Handle>,
    ) -> Self {
        layers.sort_by_key(|layer| layer.name.to_lowercase());
        layers.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));

        Self { layers, selected }
    }

    fn fuzzy_score(name: &str, input: &str) -> Option<(u8, usize, usize, String)> {
        let name_lower = name.to_lowercase();
        let query = input.trim().to_lowercase();

        if query.is_empty() {
            return None;
        }

        // Exact match.
        if name_lower == query {
            return Some((0, 0, name_lower.len(), name_lower));
        }

        // Begins with typed text.
        if name_lower.starts_with(&query) {
            return Some((1, 0, name_lower.len(), name_lower));
        }

        // A word inside the layer begins with typed text.
        for (index, word) in name_lower
            .split(|c: char| matches!(c, ' ' | '-' | '_' | '.'))
            .enumerate()
        {
            if word.starts_with(&query) {
                return Some((2, index, name_lower.len(), name_lower));
            }
        }

        // Normal substring.
        if let Some(position) = name_lower.find(&query) {
            return Some((3, position, name_lower.len(), name_lower));
        }

        // Approximate fallback: ordered subsequence.
        // Example: "cte" can still find "Corte".
        let chars: Vec<char> = name_lower.chars().collect();
        let mut cursor = 0usize;
        let mut gaps = 0usize;

        for wanted in query.chars() {
            let Some(relative) = chars[cursor..]
                .iter()
                .position(|candidate| *candidate == wanted)
            else {
                return None;
            };

            gaps += relative;
            cursor += relative + 1;
        }

        Some((4, gaps, name_lower.len(), name_lower))
    }

    fn matches(&self, input: &str) -> Vec<String> {
        let mut matches: Vec<_> = self
            .layers
            .iter()
            .filter_map(|layer| {
                Self::fuzzy_score(&layer.name, input)
                    .map(|score| (score, layer.name.clone()))
            })
            .collect();

        matches.sort_by(|a, b| a.0.cmp(&b.0));

        matches
            .into_iter()
            .map(|(_, name)| name)
            .take(8)
            .collect()
    }

    
    fn encode_layer_name(name: &str) -> String {
        let mut out = String::with_capacity(name.len() * 2);
        for byte in name.as_bytes() {
            use std::fmt::Write as _;
            let _ = write!(&mut out, "{byte:02X}");
        }
        out
    }

    pub(crate) fn decode_layer_name(encoded: &str) -> Option<String> {
        if encoded.len() % 2 != 0 {
            return None;
        }

        let mut bytes = Vec::with_capacity(encoded.len() / 2);
        let chars: Vec<char> = encoded.chars().collect();

        for i in (0..chars.len()).step_by(2) {
            let hi = chars[i].to_digit(16)?;
            let lo = chars[i + 1].to_digit(16)?;
            bytes.push(((hi << 4) | lo) as u8);
        }

        String::from_utf8(bytes).ok()
    }

fn canonical_layer(&self, input: &str) -> Option<String> {
        self.layers
            .iter()
            .find(|layer| layer.name.eq_ignore_ascii_case(input.trim()))
            .map(|layer| layer.name.clone())
    }
}

impl CadCommand for QuickLayerCommand {
    fn name(&self) -> &'static str {
        "QLAYER"
    }

    fn prompt(&self) -> String {
        if self.selected.is_empty() {
            crate::t!("QLAYER  Search current layer:").into_owned()
        } else {
            crate::tf!(
                "QLAYER  Search destination layer for {} object(s):",
                self.selected.len()
            )
            .into_owned()
        }
    }

    fn input_kind(&self) -> InputKind {
        // Layer names may legitimately contain spaces. Treat the search as one
        // complete text value so "Vista 1" is not split into "Vista" + "1".
        InputKind::FreeText
    }

    fn text_suggestions(&self, input: &str) -> Vec<CommandSuggestion> {
        self.matches(input)
            .into_iter()
            .filter_map(|name| {
                self.layers
                    .iter()
                    .find(|layer| layer.name == name)
                    .map(|layer| CommandSuggestion {
                        value: layer.name.clone(),
                        color: layer.color.clone(),
                        visible: Some(layer.visible),
                        frozen: Some(layer.frozen),
                        locked: Some(layer.locked),
                    })
            })
            .collect()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let Some(layer) = self.canonical_layer(text) else {
            return Some(CmdResult::ReportError(format!(
                "QLAYER: no layer named \"{}\".",
                text.trim()
            )));
        };

        Some(CmdResult::Relaunch(
            format!(
                "QLAYER_APPLY_HEX {}",
                Self::encode_layer_name(&layer)
            ),
            std::mem::take(&mut self.selected),
        ))
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::NeedPoint
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["QLAYER"]
});

#[cfg(test)]
mod tests {
    use super::*;

    fn command() -> QuickLayerCommand {
        QuickLayerCommand::new(
            vec![
                "Corte".into(),
                "Corte 2".into(),
                "Curvas de nivel".into(),
                "E-Proyecciones".into(),
                "Propiedad".into(),
                "Proyección".into(),
            ],
            Vec::new(),
        )
    }

    #[test]
    fn exact_match_ranks_first() {
        let matches = command().matches("corte");

        assert_eq!(matches.first().map(String::as_str), Some("Corte"));
    }

    #[test]
    fn prefix_search_finds_layer() {
        let matches = command().matches("pro");

        assert!(matches.iter().any(|name| name == "Proyección"));
        assert!(matches.iter().any(|name| name == "Propiedad"));
    }

    #[test]
    fn substring_search_finds_layer() {
        let matches = command().matches("nivel");

        assert_eq!(
            matches.first().map(String::as_str),
            Some("Curvas de nivel")
        );
    }

    #[test]
    fn approximate_search_finds_layer() {
        let matches = command().matches("cte");

        assert!(matches.iter().any(|name| name == "Corte"));
    }

    #[test]
    fn layer_name_roundtrip_preserves_spaces() {
        let original = "Vista 1";
        let encoded = QuickLayerCommand::encode_layer_name(original);
        let decoded = QuickLayerCommand::decode_layer_name(&encoded);

        assert_eq!(decoded.as_deref(), Some(original));
    }
}
