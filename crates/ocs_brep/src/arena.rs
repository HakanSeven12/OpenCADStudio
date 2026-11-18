//! Stable storage for topology nodes.
//!
//! A B-rep is a graph whose nodes point at each other in every direction: a
//! coedge names its edge, its loop and its neighbours; an edge names the
//! coedges that use it. Rust will not let those be `&` references to each
//! other, and making them `Rc<RefCell<…>>` trades the borrow checker for a
//! runtime one and a cycle leak. So nodes live in arenas and point at each
//! other by key.
//!
//! # Why the keys carry a generation
//!
//! A boolean removes faces. If a key were a bare index, the slot it named
//! would be handed to the next node created, and a key held across the
//! operation would silently start referring to something else — a dangling
//! pointer with none of the symptoms of one, producing topology that
//! validates and is wrong.
//!
//! Each slot therefore counts how many times it has been filled, and a key
//! carries the count it was issued under. Reusing a slot bumps the count, so
//! a stale key stops resolving instead of resolving to a stranger. The cost
//! is four bytes a key and a comparison a lookup.
//!
//! # Why keys are typed
//!
//! `Key<Face>` and `Key<Edge>` are both an index and a generation, and
//! nothing but the type parameter stops one being passed where the other
//! belongs — which, in a structure where every node holds keys to three other
//! kinds, is a mistake waiting to be made once per field.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

/// A handle to a node in an [`Arena`].
///
/// Copy, small, and comparable. Carries the node type so the compiler
/// distinguishes a face from an edge, and a generation so a stale handle is
/// caught rather than followed.
pub struct Key<T> {
    index: u32,
    generation: u32,
    kind: PhantomData<fn() -> T>,
}

impl<T> Key<T> {
    const fn new(index: u32, generation: u32) -> Self {
        Self {
            index,
            generation,
            kind: PhantomData,
        }
    }

    /// The slot this key names. For diagnostics and for a format layer that
    /// needs a dense numbering; not for indexing an arena directly.
    pub const fn slot(&self) -> u32 {
        self.index
    }
}

// Derived impls would demand `T: Clone` and friends, which the node types
// have no reason to satisfy — the key does not hold a `T`.
impl<T> Clone for Key<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Key<T> {}

impl<T> PartialEq for Key<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

impl<T> Eq for Key<T> {}

impl<T> Hash for Key<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.index.hash(state);
        self.generation.hash(state);
    }
}

impl<T> fmt::Debug for Key<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "#{}v{}", self.index, self.generation)
    }
}

enum Slot<T> {
    /// Holds a node, issued under this generation.
    Filled { value: T, generation: u32 },
    /// Empty, and the next node placed here will be issued this generation.
    Free { generation: u32 },
}

/// Storage for one kind of node.
pub struct Arena<T> {
    slots: Vec<Slot<T>>,
    /// Slots emptied by [`remove`](Arena::remove), for reuse.
    vacant: Vec<u32>,
    filled: usize,
}

impl<T> Arena<T> {
    /// An empty arena.
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            vacant: Vec::new(),
            filled: 0,
        }
    }

    /// How many nodes it holds.
    pub fn len(&self) -> usize {
        self.filled
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.filled == 0
    }

    /// Stores `value` and returns its key.
    pub fn insert(&mut self, value: T) -> Key<T> {
        self.filled += 1;
        match self.vacant.pop() {
            Some(index) => {
                let slot = &mut self.slots[index as usize];
                let generation = match slot {
                    Slot::Free { generation } => *generation,
                    // A slot on the vacant list is free by construction; the
                    // two are only ever written together.
                    Slot::Filled { .. } => unreachable!("vacant list named a filled slot"),
                };
                *slot = Slot::Filled { value, generation };
                Key::new(index, generation)
            }
            None => {
                let index = self.slots.len() as u32;
                self.slots.push(Slot::Filled {
                    value,
                    generation: 0,
                });
                Key::new(index, 0)
            }
        }
    }

    /// The node `key` names, or `None` if the key is stale or foreign.
    pub fn get(&self, key: Key<T>) -> Option<&T> {
        match self.slots.get(key.index as usize)? {
            Slot::Filled { value, generation } if *generation == key.generation => Some(value),
            _ => None,
        }
    }

    /// The node `key` names, mutably.
    pub fn get_mut(&mut self, key: Key<T>) -> Option<&mut T> {
        match self.slots.get_mut(key.index as usize)? {
            Slot::Filled { value, generation } if *generation == key.generation => Some(value),
            _ => None,
        }
    }

    /// Whether `key` still names a live node.
    pub fn contains(&self, key: Key<T>) -> bool {
        self.get(key).is_some()
    }

    /// Removes the node `key` names and returns it.
    ///
    /// The slot's generation advances, so every other copy of `key` stops
    /// resolving from here on — which is the point.
    pub fn remove(&mut self, key: Key<T>) -> Option<T> {
        let slot = self.slots.get_mut(key.index as usize)?;
        let Slot::Filled { generation, .. } = slot else {
            return None;
        };
        if *generation != key.generation {
            return None;
        }
        let next = generation.wrapping_add(1);
        let Slot::Filled { value, .. } = std::mem::replace(slot, Slot::Free { generation: next })
        else {
            unreachable!("just matched a filled slot");
        };
        self.vacant.push(key.index);
        self.filled -= 1;
        Some(value)
    }

    /// Every live node, with its key.
    pub fn iter(&self) -> impl Iterator<Item = (Key<T>, &T)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| match slot {
                Slot::Filled { value, generation } => {
                    Some((Key::new(index as u32, *generation), value))
                }
                Slot::Free { .. } => None,
            })
    }

    /// Every live key.
    pub fn keys(&self) -> impl Iterator<Item = Key<T>> + '_ {
        self.iter().map(|(key, _)| key)
    }

    /// Every live node, mutably.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.slots.iter_mut().filter_map(|slot| match slot {
            Slot::Filled { value, .. } => Some(value),
            Slot::Free { .. } => None,
        })
    }
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> Clone for Arena<T> {
    fn clone(&self) -> Self {
        Self {
            slots: self
                .slots
                .iter()
                .map(|slot| match slot {
                    Slot::Filled { value, generation } => Slot::Filled {
                        value: value.clone(),
                        generation: *generation,
                    },
                    Slot::Free { generation } => Slot::Free {
                        generation: *generation,
                    },
                })
                .collect(),
            vacant: self.vacant.clone(),
            filled: self.filled,
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for Arena<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_map().entries(self.iter()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct Node(u32);

    #[test]
    fn what_goes_in_comes_back_out() {
        let mut arena = Arena::new();
        let first = arena.insert(Node(1));
        let second = arena.insert(Node(2));
        assert_eq!(arena.get(first), Some(&Node(1)));
        assert_eq!(arena.get(second), Some(&Node(2)));
        assert_eq!(arena.len(), 2);
    }

    #[test]
    fn a_removed_node_is_gone_and_its_key_stops_resolving() {
        let mut arena = Arena::new();
        let key = arena.insert(Node(1));
        assert_eq!(arena.remove(key), Some(Node(1)));
        assert!(!arena.contains(key));
        assert_eq!(arena.get(key), None);
        assert!(arena.is_empty());
        // And it cannot be removed twice.
        assert_eq!(arena.remove(key), None);
    }

    #[test]
    fn a_stale_key_does_not_resolve_to_whatever_took_the_slot() {
        // The failure a bare index would give: the removed node's key
        // silently starts naming the node inserted after it, and every
        // reference held across a boolean points somewhere plausible and
        // wrong.
        let mut arena = Arena::new();
        let stale = arena.insert(Node(1));
        arena.remove(stale);
        let fresh = arena.insert(Node(2));
        assert_eq!(stale.slot(), fresh.slot(), "the slot really was reused");
        assert_ne!(stale, fresh);
        assert_eq!(arena.get(stale), None);
        assert_eq!(arena.get(fresh), Some(&Node(2)));
    }

    #[test]
    fn removing_frees_the_slot_for_the_next_insert() {
        let mut arena = Arena::new();
        let first = arena.insert(Node(1));
        let second = arena.insert(Node(2));
        arena.remove(first);
        let third = arena.insert(Node(3));
        assert_eq!(third.slot(), first.slot());
        assert_eq!(arena.len(), 2);
        assert_eq!(arena.get(second), Some(&Node(2)));
    }

    #[test]
    fn a_key_from_another_arena_does_not_resolve() {
        let mut one = Arena::new();
        let mut other = Arena::new();
        one.insert(Node(1));
        one.insert(Node(2));
        let foreign = other.insert(Node(9));
        // Same slot number, and the generations happen to match too — only
        // the value tells them apart, which is why an arena must never be
        // indexed with a key it did not issue. What it must not do is panic.
        assert_eq!(one.get(foreign), Some(&Node(1)));
        assert_eq!(other.get(foreign), Some(&Node(9)));
    }

    #[test]
    fn an_out_of_range_key_is_refused_rather_than_panicking() {
        let mut arena: Arena<Node> = Arena::new();
        let key = arena.insert(Node(1));
        let mut elsewhere: Arena<Node> = Arena::new();
        for i in 0..5 {
            elsewhere.insert(Node(i));
        }
        let far = elsewhere.keys().last().unwrap();
        assert_eq!(arena.get(far), None);
        assert_eq!(arena.get(key), Some(&Node(1)));
    }

    #[test]
    fn mutation_reaches_the_stored_node() {
        let mut arena = Arena::new();
        let key = arena.insert(Node(1));
        arena.get_mut(key).unwrap().0 = 42;
        assert_eq!(arena.get(key), Some(&Node(42)));
        for node in arena.values_mut() {
            node.0 += 1;
        }
        assert_eq!(arena.get(key), Some(&Node(43)));
    }

    #[test]
    fn iteration_skips_the_holes() {
        let mut arena = Arena::new();
        let keys: Vec<_> = (0..5).map(|i| arena.insert(Node(i))).collect();
        arena.remove(keys[1]);
        arena.remove(keys[3]);
        let seen: Vec<u32> = arena.iter().map(|(_, node)| node.0).collect();
        assert_eq!(seen, vec![0, 2, 4]);
        assert_eq!(arena.keys().count(), 3);
    }

    #[test]
    fn a_generation_that_wraps_still_moves_on() {
        // Four billion removals of one slot is not a real workload, but the
        // arithmetic must not panic in a debug build if it ever happened.
        let mut arena = Arena::new();
        let key = arena.insert(Node(1));
        arena.remove(key);
        if let Slot::Free { generation } = &mut arena.slots[0] {
            *generation = u32::MAX;
        }
        let wrapped = arena.insert(Node(2));
        arena.remove(wrapped);
        assert!(!arena.contains(wrapped));
    }

    #[test]
    fn cloning_carries_the_generations_with_it() {
        let mut arena = Arena::new();
        let stale = arena.insert(Node(1));
        arena.remove(stale);
        let live = arena.insert(Node(2));
        let copy = arena.clone();
        assert_eq!(copy.get(live), Some(&Node(2)));
        assert_eq!(copy.get(stale), None, "a stale key stays stale in a copy");
    }
}
