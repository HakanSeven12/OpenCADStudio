//! Iterative solvers that drive a [`SubSystem`](crate::subsystem::SubSystem)'s
//! residual toward zero by moving its free parameters.
//!
//! Ported from planegcs's three algorithms in `GCS.cpp`
//! (`System::solve_DL`/`solve_LM`/`solve_BFGS`), one per submodule. Common
//! to all of them: the C++ solves against its own scratch parameter copy
//! (`SubSystem::pvals`, populated by `redirectParams()`) and leaves the
//! *real* parameters untouched until a caller separately calls
//! `applySolution()` after checking the return status. This port has no
//! such scratch copy (see `subsystem.rs`'s module doc) — `SubSystem::
//! set_params` always writes straight into the live [`ParamStore`
//! ](crate::util::ParamStore) — so each solver here takes that snapshot
//! itself, via [`ParamStore::redirect`](crate::util::ParamStore::redirect)
//! before iterating, and either leaves the store holding the converged
//! values (success) or calls
//! [`ParamStore::revert`](crate::util::ParamStore::revert) to undo every
//! trial step (failure) — the same commit-on-success/discard-on-failure
//! contract, just self-contained instead of split across two calls.

pub mod bfgs;
pub mod dogleg;
pub mod lm;

/// Mirrors planegcs's `SolveStatus` (`GCS.h`). Dogleg and LM only ever
/// produce two of its three C++ values — every non-1 stop code (max-
/// iterations, no-progress, diverging, NaN) collapses to `Failed`, matching
/// their own `return (stop == 1) ? Success : Failed;`. BFGS is the one
/// solver that distinguishes a genuine `Converged` (step size below
/// tolerance, but the residual itself isn't yet below `smallF`) from a full
/// `Success` — see its own return statement in `GCS.cpp` — so this keeps
/// that third variant rather than folding it into `Failed`, which would
/// misrepresent what is usually still a usable result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolveStatus {
    Success,
    Converged,
    Failed,
}
