//! The draw guard: a caught panic keeps its text one time, and the data
//! keys of failed frames are kept, at most eight.

use super::draw_guard::{panicked, take_notice, DrawFaults};

#[test]
fn a_panic_in_a_draw_is_caught_and_its_text_is_kept_one_time() {
    let length = 7.5e-5_f32;
    assert!(panicked(|| panic!("dash end {length} is too short")));
    assert_eq!(
        take_notice().as_deref(),
        Some("dash end 0.000075 is too short")
    );
    assert_eq!(take_notice(), None, "the command line shows it one time");
}

#[test]
fn a_draw_that_returns_is_not_a_panic() {
    assert!(!panicked(|| {}));
    assert_eq!(take_notice(), None);
}

#[test]
fn the_faults_keep_the_newest_eight_keys() {
    let faults = DrawFaults::default();
    for key in 1..=9 {
        faults.add(key);
    }
    faults.add(9);
    assert!(!faults.has(1), "the oldest key goes first");
    assert!((2..=9).all(|key| faults.has(key)), "the eight newest stay");
    faults.add(10);
    assert!(!faults.has(2), "adding 9 again did not take a place");
}
