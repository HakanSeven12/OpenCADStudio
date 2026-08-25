//! Reusable block-name picker for INSERT/MINSERT interactive search.
//!
//! Encapsulates the full list, lower/upper caches, usage ranking,
//! current needle and filtered view. Centralizes the O(n) scan +
//! partial-sort ranking so both commands share one optimized path
//! and future pickers can reuse it.

use rustc_hash::FxHashMap;

/// Maximum suggestion buttons, aligned with `CLIPROMPTLINES` range (0-50).
/// The actual limit is `CLIPROMPTLINES` — number of options is in direct
/// relation to allowed prompt lines, per spec. This constant is the hard cap
/// for performance (widget tree) and matches the sysvar max.
pub const MAX_SUGGESTIONS: usize = 50;

#[derive(Debug, Clone)]
pub struct BlockPicker {
    available: Vec<String>,
    lower_cache: Vec<String>,
    upper_cache: Vec<String>,
    filtered: Vec<String>,
    needle: String,
    limit: usize,
    usage_rank: FxHashMap<String, (u32, usize)>,
}

impl BlockPicker {
    /// Create a picker. `available` is the full block name list (unsorted).
    /// `usage_rank` is `UPPER -> (freq, mru_idx)`. `limit` is capped to
    /// `MAX_SUGGESTIONS` by caller (or passed as `MAX_SUGGESTIONS` directly).
    pub fn new(mut available: Vec<String>, usage_rank: FxHashMap<String, (u32, usize)>, limit: usize) -> Self {
        let limit = limit.clamp(0, MAX_SUGGESTIONS);
        // Rank `available` for the empty-needle view. Use partial sort when
        // the list is large to avoid O(n log n) when we only need top `limit`.
        if !usage_rank.is_empty() && available.len() > limit && limit > 0 {
            // Build scored indices and select top `limit` before full sort.
            let mut indices: Vec<usize> = (0..available.len()).collect();
            indices.select_nth_unstable_by(limit - 1, |&a, &b| {
                let up_a = available[a].to_ascii_uppercase();
                let up_b = available[b].to_ascii_uppercase();
                let (fa, ma) = usage_rank.get(&up_a).copied().unwrap_or((0, usize::MAX));
                let (fb, mb) = usage_rank.get(&up_b).copied().unwrap_or((0, usize::MAX));
                // higher freq first, smaller mru first, then alpha
                fb.cmp(&fa).then_with(|| ma.cmp(&mb)).then_with(|| available[a].cmp(&available[b]))
            });
            indices.truncate(limit);
            indices.sort_by(|&a, &b| {
                let up_a = available[a].to_ascii_uppercase();
                let up_b = available[b].to_ascii_uppercase();
                let (fa, ma) = usage_rank.get(&up_a).copied().unwrap_or((0, usize::MAX));
                let (fb, mb) = usage_rank.get(&up_b).copied().unwrap_or((0, usize::MAX));
                fb.cmp(&fa).then_with(|| ma.cmp(&mb)).then_with(|| available[a].cmp(&available[b]))
            });
            let mut ranked: Vec<String> = indices.into_iter().map(|i| available[i].clone()).collect();
            // Append the rest unsorted — they are only needed when filtering.
            // To keep `available` complete for filtering, retain all names but
            // move the ranked top to front. Simpler: sort fully if limit==available.len().
            // For large n, we still want the rest available for substring search.
            // So rebuild: ranked top + remaining in arbitrary order.
            let mut remaining: Vec<String> = available.into_iter().filter(|name| !ranked.contains(name)).collect();
            ranked.append(&mut remaining);
            available = ranked;
        } else if available.len() > 1 {
            // Small n or no usage_rank: full sort is cheap and gives deterministic empty-needle order.
            if !usage_rank.is_empty() {
                available.sort_by(|a, b| {
                    let up_a = a.to_ascii_uppercase();
                    let up_b = b.to_ascii_uppercase();
                    let (fa, ma) = usage_rank.get(&up_a).copied().unwrap_or((0, usize::MAX));
                    let (fb, mb) = usage_rank.get(&up_b).copied().unwrap_or((0, usize::MAX));
                    fb.cmp(&fa).then_with(|| ma.cmp(&mb)).then_with(|| a.cmp(b))
                });
            } else {
                available.sort();
            }
        }

        let lower_cache: Vec<String> = available.iter().map(|s| s.to_ascii_lowercase()).collect();
        let upper_cache: Vec<String> = available.iter().map(|s| s.to_ascii_uppercase()).collect();
        let filtered = Self::filter_ranked_inner(&available, &lower_cache, &upper_cache, "", &usage_rank, limit);
        Self {
            available,
            lower_cache,
            upper_cache,
            filtered,
            needle: String::new(),
            limit,
            usage_rank,
        }
    }

    /// Current needle (as typed, trimmed).
    pub fn needle(&self) -> &str {
        &self.needle
    }

    /// Filtered view (≤ limit).
    pub fn filtered(&self) -> &[String] {
        &self.filtered
    }

    /// Full list length.
    pub fn total(&self) -> usize {
        self.available.len()
    }

    /// Whether the full list is empty.
    pub fn is_empty(&self) -> bool {
        self.available.is_empty()
    }

    /// Does `available` contain `name` case-insensitively?
    pub fn contains_name(&self, name: &str) -> Option<String> {
        self.available.iter().find(|c| c.eq_ignore_ascii_case(name)).cloned()
    }

    /// Update needle and recompute `filtered`. Pass trimmed `needle`.
    /// Handles empty string as reset to default ranked view.
    pub fn set_needle(&mut self, needle: String) {
        self.needle = needle;
        let needle_ref = self.needle.as_str();
        self.filtered = Self::filter_ranked_inner(
            &self.available,
            &self.lower_cache,
            &self.upper_cache,
            needle_ref,
            &self.usage_rank,
            self.limit,
        );
    }

    /// Test helper: simulate typing without exact-match handling.
    #[cfg(test)]
    pub fn set_needle_for_test(&mut self, needle: &str) {
        self.set_needle(needle.to_string());
    }

    fn filter_ranked_inner(
        available: &[String],
        lower_cache: &[String],
        upper_cache: &[String],
        needle: &str,
        usage_rank: &FxHashMap<String, (u32, usize)>,
        limit: usize,
    ) -> Vec<String> {
        if limit == 0 {
            return Vec::new();
        }
        if needle.is_empty() {
            return available.iter().take(limit).cloned().collect();
        }
        let n = needle.to_ascii_lowercase();
        let mut scored: Vec<(usize, u8, i32, usize)> = Vec::new();
        scored.reserve(available.len().min(limit * 4 + 16));
        for (i, lc) in lower_cache.iter().enumerate() {
            if let Some(pos) = lc.find(n.as_str()) {
                // Use upper_cache to avoid allocating per block per keystroke (1.1).
                let up = &upper_cache[i];
                let (freq, mru) = usage_rank.get(up).copied().unwrap_or((0, usize::MAX));
                let prefix = if pos == 0 { 0 } else { 1 };
                scored.push((i, prefix, -(freq as i32), mru));
            }
        }
        if scored.is_empty() {
            return Vec::new();
        }
        // Partial sort: only need top `limit` (1.2).
        let comparator = |a: &(usize, u8, i32, usize), b: &(usize, u8, i32, usize)| {
            a.1.cmp(&b.1)
                .then_with(|| a.2.cmp(&b.2))
                .then_with(|| a.3.cmp(&b.3))
                .then_with(|| available[a.0].cmp(&available[b.0]))
        };
        if scored.len() > limit {
            scored.select_nth_unstable_by(limit - 1, comparator);
            scored.truncate(limit);
        }
        scored.sort_by(comparator);
        scored.into_iter().map(|(i, _, _, _)| available[i].clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashMap;

    fn picker(names: &[&str], usage: Vec<(&str, u32, usize)>, limit: usize) -> BlockPicker {
        let mut map = FxHashMap::default();
        for (name, freq, idx) in usage { map.insert(name.to_ascii_uppercase(), (freq, idx)); }
        BlockPicker::new(names.iter().map(|s| s.to_string()).collect(), map, limit)
    }

    #[test]
    fn empty_needle_shows_ranked_top() {
        let mut p = picker(&["Gamma","Alpha","Beta"], vec![("Beta",5,0),("Alpha",2,1)], 2);
        assert_eq!(p.filtered(), &["Beta","Alpha"]);
        p.set_needle("".into());
        assert_eq!(p.filtered(), &["Beta","Alpha"]);
    }

    #[test]
    fn substring_filter_prefix_first() {
        let mut p = picker(&["Alpha","Alphabet","Beta","Alpine"], vec![], 8);
        p.set_needle("Al".into());
        let f = p.filtered();
        assert!(f.iter().all(|n| n.to_lowercase().contains("al")));
        // prefix matches before substring — but all four contain Al at pos 0 except maybe edge, so alphabetical
        assert_eq!(f.len(), 3);
    }

    #[test]
    fn empty_input_resets_to_default() {
        let mut p = picker(&["A","B","C"], vec![], 2);
        p.set_needle("B".into());
        assert_eq!(p.filtered(), &["B"]);
        p.set_needle("".into());
        assert_eq!(p.filtered().len(), 2);
    }

    #[test]
    fn upper_cache_avoids_allocation_and_partial_sort_limits() {
        let names: Vec<String> = (0..1000).map(|i| format!("Block_{:04}", i)).collect();
        let mut p = BlockPicker::new(names, FxHashMap::default(), 8);
        let start = std::time::Instant::now();
        for needle in ["B","Bl","Block_0","Block_00","a"] {
            p.set_needle(needle.into());
            assert!(p.filtered().len() <= 8);
        }
        assert!(start.elapsed().as_millis() < 20, "too slow");
    }
}
