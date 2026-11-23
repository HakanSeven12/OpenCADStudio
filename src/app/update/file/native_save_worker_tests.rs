use super::*;

use super::native_save_worker;
use iced::futures::{executor::block_on, task::noop_waker};
use std::{future::Future, sync::mpsc, task::Context, time::Duration};

#[test]
fn waiting_for_save_yields_before_the_worker_finishes() {
    let (release, released) = mpsc::channel();
    let (polled, polling) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        let mut future = Box::pin(native_save_worker(move || {
            released.recv_timeout(Duration::from_secs(5)).unwrap();
            42
        }));
        let waker = noop_waker();
        let mut context = Context::from_waker(&waker);
        let pending = future.as_mut().poll(&mut context).is_pending();
        polled.send(pending).unwrap();
        block_on(future).unwrap()
    });
    // Release even on failure, so a blocking-wait regression cannot leave
    // the test worker stuck waiting for its own assertion to finish.
    let first_poll = polling.recv_timeout(Duration::from_secs(2));
    release.send(()).unwrap();
    assert_eq!(waiter.join().unwrap(), 42);
    assert_eq!(first_poll, Ok(true));
}

#[test]
fn save_worker_preserves_returned_failure() {
    let result = block_on(native_save_worker(|| Err::<(), _>("write failed"))).unwrap();
    assert_eq!(result, Err("write failed"));
}

#[test]
fn save_worker_panic_becomes_a_save_failure() {
    let error = block_on(native_save_worker(|| panic!("worker exploded"))).unwrap_err();
    assert_eq!(error.to_string(), "save worker panicked");
}
