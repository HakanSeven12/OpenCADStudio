//! Parameter storage.
//!
//! planegcs represents every solver parameter as a raw `double*` aliased
//! between geometry storage and the solver's working copy, with
//! `redirectParams`/`revertParams` swapping what those pointers point at
//! during an iteration. Rust disallows that aliasing, so parameters are
//! plain indices (`ParamId`) into a [`ParamStore`], and "redirect" becomes
//! snapshotting the store's values before an iterative solve mutates them
//! in place, with "revert" restoring that snapshot on failure.

/// An index into a [`ParamStore`]. Cheap to copy, holds no value itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ParamId(u32);

/// The solver's parameter values, addressed by [`ParamId`].
///
/// A parameter marked `driven` is a fixed input (e.g. a dimension value)
/// the solver must read but never write.
#[derive(Debug, Clone, Default)]
pub struct ParamStore {
    values: Vec<f64>,
    driven: Vec<bool>,
}

/// A saved copy of a [`ParamStore`]'s values, for [`ParamStore::revert`].
///
/// Only valid against the store it was taken from; reverting into a store
/// with a different parameter count is a programmer error and panics.
#[derive(Debug, Clone)]
pub struct Snapshot(Vec<f64>);

impl ParamStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Adds a parameter and returns the id to address it by.
    pub fn add(&mut self, value: f64, driven: bool) -> ParamId {
        let id = ParamId(self.values.len() as u32);
        self.values.push(value);
        self.driven.push(driven);
        id
    }

    pub fn get(&self, id: ParamId) -> f64 {
        self.values[id.0 as usize]
    }

    /// Overwrites a parameter's value regardless of its `driven` flag.
    ///
    /// The solver itself must not call this on a driven parameter — that
    /// check belongs to the constraint/subsystem layer, which knows which
    /// parameters it is allowed to move. This method stays unconditional so
    /// setting up or re-driving a sketch (the one place driven values do
    /// change) does not need a separate code path.
    pub fn set(&mut self, id: ParamId, value: f64) {
        self.values[id.0 as usize] = value;
    }

    pub fn is_driven(&self, id: ParamId) -> bool {
        self.driven[id.0 as usize]
    }

    pub fn set_driven(&mut self, id: ParamId, driven: bool) {
        self.driven[id.0 as usize] = driven;
    }

    /// Captures the current values so a failed solve can [`revert`](Self::revert).
    pub fn redirect(&self) -> Snapshot {
        Snapshot(self.values.clone())
    }

    /// Restores values captured by [`redirect`](Self::redirect).
    ///
    /// Panics if `snapshot` was not taken from this store (parameter count
    /// mismatch) — that pairing is a solver-internal invariant, not
    /// something callers should recover from.
    pub fn revert(&mut self, snapshot: &Snapshot) {
        assert_eq!(
            self.values.len(),
            snapshot.0.len(),
            "snapshot taken from a store with a different parameter count"
        );
        self.values.copy_from_slice(&snapshot.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_get_set_round_trip() {
        let mut store = ParamStore::new();
        let a = store.add(1.0, false);
        let b = store.add(2.0, true);
        assert_eq!(store.get(a), 1.0);
        assert_eq!(store.get(b), 2.0);
        assert!(!store.is_driven(a));
        assert!(store.is_driven(b));

        store.set(a, 5.0);
        assert_eq!(store.get(a), 5.0);
    }

    #[test]
    fn redirect_then_revert_restores_original_values() {
        let mut store = ParamStore::new();
        let a = store.add(1.0, false);
        let b = store.add(2.0, false);

        let snapshot = store.redirect();
        store.set(a, 100.0);
        store.set(b, 200.0);
        assert_eq!(store.get(a), 100.0);
        assert_eq!(store.get(b), 200.0);

        store.revert(&snapshot);
        assert_eq!(store.get(a), 1.0);
        assert_eq!(store.get(b), 2.0);
    }

    #[test]
    fn revert_after_no_mutation_is_a_no_op() {
        let mut store = ParamStore::new();
        let a = store.add(3.5, false);
        let snapshot = store.redirect();
        store.revert(&snapshot);
        assert_eq!(store.get(a), 3.5);
    }

    #[test]
    #[should_panic(expected = "different parameter count")]
    fn revert_with_mismatched_snapshot_panics() {
        let mut store = ParamStore::new();
        store.add(1.0, false);
        let snapshot = store.redirect();

        let mut other = ParamStore::new();
        other.add(1.0, false);
        other.add(2.0, false);
        other.revert(&snapshot);
    }

    #[test]
    fn set_driven_can_toggle_the_flag() {
        let mut store = ParamStore::new();
        let a = store.add(1.0, false);
        assert!(!store.is_driven(a));
        store.set_driven(a, true);
        assert!(store.is_driven(a));
    }
}
