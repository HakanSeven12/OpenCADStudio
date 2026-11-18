//! The plugin-side revision cursor (ADR-013): a PURE value type for the
//! publish/patch revision arithmetic every publish frame hand-rolls
//! (`base = head; successor = head + 1`) and the stale-frontier realign a
//! refusal report's `current_revision` enables.
//!
//! RULING (ADR-013, `project-management/DECISIONS/
//! ADR-013-revision-cursor-ruling.md`, binding): this type is bookkeeping
//! ONLY. Exactly four behavioral methods — [`RevisionCursor::head`],
//! [`RevisionCursor::successor`], [`RevisionCursor::advance`], and
//! [`RevisionCursor::realign`] — plus a bare constructor. NO retry, NO
//! re-send, NO back-off, NO automatic realignment, NO hidden state machine,
//! NO I/O, NO allocation. What to DO after a refusal — re-derive and
//! re-base, defer, or abort — remains 100% plugin-owned policy: the host
//! never learns plugin semantics, and this crate must not silently acquire
//! policy either. Any future temptation (auto-retry, auto-realign, send
//! hooks) is a NEW API surface and a NEW ADR. A source-scan test in this
//! module pins that surface.

/// The plugin-side revision cursor: one `u64` frontier plus the four
/// ruling methods (see the [module docs](self) for the ADR-013 scope).
///
/// Usage shape (per ADR-013's context — the lines this centralizes):
///
/// ```no_run
/// use ocs_plugin_api::revision::RevisionCursor;
///
/// let mut cursor = RevisionCursor::new(1);
/// let base = cursor.head();       // the base revision the patch frame declares
/// let next = cursor.successor();  // == base + 1
/// // … publish/patch confirmed …
/// cursor.advance();               // head = successor
/// // … a publish is refused; the PatchRejectReport names current_revision …
/// cursor.realign(1);              // head = reported — the plugin DECIDES to call this
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RevisionCursor {
    /// The confirmed frontier (private; the only state).
    head: u64,
}

impl RevisionCursor {
    /// A cursor whose frontier starts at `head` (the bare constructor; the
    /// genesis value is the plugin's business, not a policy).
    pub const fn new(head: u64) -> Self {
        Self { head }
    }

    /// The confirmed frontier: the base revision the next publish/patch
    /// frame declares.
    pub const fn head(&self) -> u64 {
        self.head
    }

    /// `head() + 1`: the successor the next confirmed publish moves to.
    /// Plain arithmetic — a revision approaching `u64::MAX` is a plugin
    /// defect generations before it is an overflow.
    pub const fn successor(&self) -> u64 {
        self.head + 1
    }

    /// Confirms the successor: `head = successor()`. The plugin calls this
    /// after an APPLIED publish/patch ack; it is never called for you.
    pub fn advance(&mut self) {
        self.head = self.successor();
    }

    /// Sets `head = reported` — the stale-frontier realign from a refusal
    /// report's `current_revision`. EXPLICIT by ruling: the cursor never
    /// realigns itself, never sees a response, and never decides anything.
    pub fn realign(&mut self, reported: u64) {
        self.head = reported;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successor_is_head_plus_one() {
        for head in [0u64, 1, 41, u64::MAX - 1] {
            let cursor = RevisionCursor::new(head);
            assert_eq!(cursor.successor(), head + 1);
            assert_eq!(cursor.head(), head, "reading never mutates");
        }
    }

    #[test]
    fn advance_moves_head_to_the_successor() {
        let mut cursor = RevisionCursor::new(3);
        cursor.advance();
        assert_eq!(cursor.head(), 4);
        cursor.advance();
        assert_eq!(cursor.head(), 5);
        assert_eq!(cursor.successor(), 6);
    }

    #[test]
    fn realign_sets_head_from_a_refusal_reports_current_revision() {
        // The ADR-013 sequence: publishes advance the cursor; a refused
        // frame reports the host's current_revision; the plugin DECIDES to
        // realign from it. (The report itself is wire data — this pin is
        // about the cursor's part: head becomes exactly `reported`.)
        let mut cursor = RevisionCursor::new(1);
        cursor.advance();
        cursor.advance();
        assert_eq!(cursor.head(), 3);

        // PatchRejectReport { current_revision: Some(2), .. } → realign(2).
        cursor.realign(2);
        assert_eq!(cursor.head(), 2);
        assert_eq!(cursor.successor(), 3, "the re-based frame is ready");

        // Realigning is absolute (idempotent on the same report value).
        cursor.realign(2);
        assert_eq!(cursor.head(), 2);
    }

    #[test]
    fn the_cursor_is_a_pure_value_type() {
        // Copy/Clone/Eq only — no interior mutability, no handles, nothing
        // to do I/O with. (If this stops compiling, the type grew beyond
        // ADR-013's "pure value type".)
        fn assert_value_type<T: Copy + Clone + PartialEq + Eq + std::fmt::Debug>() {}
        assert_value_type::<RevisionCursor>();

        let cursor = RevisionCursor::new(7);
        let moved = cursor; // Copy — a read never borrows state
        assert_eq!(cursor, moved);
        assert_eq!(moved.head(), 7);
    }

    #[test]
    fn the_api_surface_is_exactly_the_ruling_constructor_and_four_methods() {
        // ADR-013's policy-creep guard: the impl block exposes exactly
        // `new` + `head`/`successor`/`advance`/`realign` — nothing else.
        // Scanned from this file's source so an added method fails HERE,
        // with the ruling quoted, not in review.
        let source = include_str!("revision.rs");
        let mut methods: Vec<&str> = Vec::new();
        for line in source.lines() {
            let line = line.trim_start();
            let decl = line
                .strip_prefix("pub fn ")
                .or_else(|| line.strip_prefix("pub const fn "));
            if let Some(rest) = decl {
                let name = rest.split(['(', '<', ' ']).next().unwrap_or("");
                if !name.is_empty() {
                    methods.push(name);
                }
            }
        }
        methods.sort_unstable();
        assert_eq!(
            methods,
            vec!["advance", "head", "new", "realign", "successor"],
            "ADR-013: exactly the bare constructor plus the four ruling \
             methods — anything richer (retry, re-send, auto-realign, I/O) \
             is policy creep and needs a NEW ADR"
        );
    }
}
