//! The names of the open phases.

use super::{
    open_phase_name, OPEN_PHASE_CACHING, OPEN_PHASE_FINALIZING, OPEN_PHASE_PARSING,
    OPEN_PHASE_READING, OPEN_PHASE_XREF,
};

#[test]
fn each_open_phase_has_its_name() {
    assert_eq!(open_phase_name(OPEN_PHASE_READING), "reading");
    assert_eq!(open_phase_name(OPEN_PHASE_PARSING), "parsing");
    assert_eq!(open_phase_name(OPEN_PHASE_XREF), "references");
    assert_eq!(open_phase_name(OPEN_PHASE_CACHING), "derived-caches");
    assert_eq!(open_phase_name(OPEN_PHASE_FINALIZING), "finalizing");
    assert_eq!(open_phase_name(5), "unknown");
}
