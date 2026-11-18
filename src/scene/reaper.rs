//! Off-path teardown for evicted resident wire sets.
//!
//! Dropping a large wire set walks every wire's name `String`, layer `Arc`
//! and point `Vec` — tens of milliseconds of deallocation that is
//! allocator-independent (measured ~39 ms for 200k wires on both the system
//! heap and mimalloc). That cost must not run on the rebuild path, so evicted
//! sets are handed to a dedicated thread that drops them in parallel with the
//! next frame's work.

use std::sync::mpsc::{self, Sender};
use std::sync::OnceLock;

/// A handle to a drop thread. Values handed to it are dropped on that thread,
/// not on the caller's.
pub(crate) struct Reaper {
    tx: Sender<Box<dyn Send + 'static>>,
}

impl Reaper {
    pub(crate) fn reap(&self, value: impl Send + 'static) {
        // A failed send means the drop thread is gone; dropping the box here
        // is then exactly the synchronous teardown this replaces.
        let _ = self.tx.send(Box::new(value));
    }
}

static GLOBAL: OnceLock<Reaper> = OnceLock::new();

pub(crate) fn global() -> &'static Reaper {
    GLOBAL.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Box<dyn Send + 'static>>();
        let _ = std::thread::Builder::new()
            .name("ocs-wire-reaper".to_string())
            .spawn(move || {
                while let Ok(value) = rx.recv() {
                    drop(value);
                }
            });
        Reaper { tx }
    })
}

/// Hand an evicted value to the global drop thread.
pub(crate) fn reap(value: impl Send + 'static) {
    global().reap(value);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    struct CountDrop(Arc<AtomicUsize>);
    impl Drop for CountDrop {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn reap_defers_drop_to_the_receiver() {
        let (tx, rx) = mpsc::channel::<Box<dyn Send + 'static>>();
        let reaper = Reaper { tx };
        let count = Arc::new(AtomicUsize::new(0));
        reaper.reap(CountDrop(count.clone()));
        // Nothing has drained the channel, so the value must still be alive.
        assert_eq!(count.load(Ordering::SeqCst), 0);
        let held = rx.recv().unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 0);
        drop(held);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn global_reaper_drops_on_another_thread() {
        struct SignalDrop(mpsc::Sender<std::thread::ThreadId>);
        impl Drop for SignalDrop {
            fn drop(&mut self) {
                let _ = self.0.send(std::thread::current().id());
            }
        }
        let (tx, rx) = mpsc::channel();
        let caller = std::thread::current().id();
        reap(SignalDrop(tx));
        let drop_thread = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_ne!(drop_thread, caller);
    }
}