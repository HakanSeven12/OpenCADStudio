// Parallel-iterator prelude that degrades to sequential iteration on the web.
//
// Native builds re-export `rayon::prelude`. wasm32 has no threads, so the same
// call sites (`.par_iter()`, `.into_par_iter()`, `.flat_map_iter()`) resolve to
// plain sequential `std` iterators instead — identical results, just single
// threaded. Bring it in with `use crate::par::prelude::*;`.

#[cfg(not(target_arch = "wasm32"))]
pub mod prelude {
    pub use rayon::prelude::*;
}

#[cfg(target_arch = "wasm32")]
pub mod prelude {
    use std::collections::{HashMap, HashSet};

    /// `.par_iter()` over a shared reference → sequential `std` iterator.
    pub trait ParRefShim<'a> {
        type Iter;
        fn par_iter(&'a self) -> Self::Iter;
    }

    impl<'a, T: 'a> ParRefShim<'a> for [T] {
        type Iter = core::slice::Iter<'a, T>;
        fn par_iter(&'a self) -> Self::Iter {
            self.iter()
        }
    }

    impl<'a, T: 'a> ParRefShim<'a> for Vec<T> {
        type Iter = core::slice::Iter<'a, T>;
        fn par_iter(&'a self) -> Self::Iter {
            self.iter()
        }
    }

    impl<'a, K: 'a, V: 'a, S> ParRefShim<'a> for HashMap<K, V, S> {
        type Iter = std::collections::hash_map::Iter<'a, K, V>;
        fn par_iter(&'a self) -> Self::Iter {
            self.iter()
        }
    }

    impl<'a, T: 'a, S> ParRefShim<'a> for HashSet<T, S> {
        type Iter = std::collections::hash_set::Iter<'a, T>;
        fn par_iter(&'a self) -> Self::Iter {
            self.iter()
        }
    }

    /// `.into_par_iter()` → sequential `into_iter()`.
    pub trait IntoParShim {
        type IntoIter;
        fn into_par_iter(self) -> Self::IntoIter;
    }

    impl<I: IntoIterator> IntoParShim for I {
        type IntoIter = I::IntoIter;
        fn into_par_iter(self) -> Self::IntoIter {
            self.into_iter()
        }
    }

    /// rayon's `flat_map_iter` has no `std` equivalent name; map it to
    /// `flat_map`.
    pub trait IterParExt: Iterator + Sized {
        fn flat_map_iter<U, F>(self, f: F) -> std::iter::FlatMap<Self, U, F>
        where
            F: FnMut(Self::Item) -> U,
            U: IntoIterator,
        {
            self.flat_map(f)
        }
    }

    impl<I: Iterator> IterParExt for I {}
}

#[cfg(not(target_arch = "wasm32"))]
pub use rayon::join;

#[cfg(target_arch = "wasm32")]
pub fn join<A, B>(a: impl FnOnce() -> A, b: impl FnOnce() -> B) -> (A, B) {
    (a(), b())
}
