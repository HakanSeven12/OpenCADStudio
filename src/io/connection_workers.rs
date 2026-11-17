//! Bounded, reusable workers for blocking local control connections.
//!
//! A slow peer occupies one worker. Admission includes queued and running jobs,
//! and a permit is returned even when the connection handler unwinds.

use std::io;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc::{self, SyncSender},
    Arc, Mutex,
};

type Task = Box<dyn FnOnce() + Send + 'static>;

struct Permit(Arc<AtomicUsize>);

impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Job {
    task: Task,
    permit: Permit,
}

pub(crate) struct ConnectionWorkers {
    sender: SyncSender<Job>,
    receiver: Arc<Mutex<mpsc::Receiver<Job>>>,
    active: Arc<AtomicUsize>,
    started: Mutex<usize>,
    name: String,
    limit: usize,
}

impl ConnectionWorkers {
    pub(crate) fn new(name: &str, limit: usize) -> io::Result<Self> {
        if limit == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "worker limit is zero",
            ));
        }
        let (sender, receiver) = mpsc::sync_channel::<Job>(limit);
        let receiver = Arc::new(Mutex::new(receiver));
        Ok(Self {
            sender,
            receiver,
            active: Arc::new(AtomicUsize::new(0)),
            started: Mutex::new(0),
            name: name.to_owned(),
            limit,
        })
    }

    /// Reject excess connections immediately, retaining the transport's bound.
    pub(crate) fn try_execute(&self, task: impl FnOnce() + Send + 'static) -> bool {
        let previous =
            match self
                .active
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                    (active < self.limit).then_some(active + 1)
                }) {
                Ok(previous) => previous,
                Err(_) => return false,
            };
        let permit = Permit(Arc::clone(&self.active));
        {
            // Start only as much concurrency as clients have actually needed.
            // Idle listeners create no worker threads, and existing workers
            // stay available for later requests instead of churning TLS state.
            let mut started = self.started.lock().unwrap_or_else(|e| e.into_inner());
            if *started < previous + 1 {
                let receiver = Arc::clone(&self.receiver);
                let worker = std::thread::Builder::new()
                    .name(format!("{}-{}", self.name, *started))
                    .spawn(move || loop {
                        // Never hold the receiver lock while executing a handler.
                        let next = receiver.lock().unwrap_or_else(|e| e.into_inner()).recv();
                        let Ok(Job { task, permit }) = next else {
                            break;
                        };
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task));
                        drop(permit);
                    });
                if let Err(error) = worker {
                    eprintln!("{}: cannot start connection worker: {error}", self.name);
                    return false;
                }
                *started += 1;
            }
        }
        let job = Job {
            task: Box::new(task),
            permit,
        };
        // Rejected jobs drop their permit and captured connection here.
        self.sender.try_send(job).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::time::{Duration, Instant};

    fn wait_idle(workers: &ConnectionWorkers) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while workers.active.load(Ordering::Acquire) != 0 {
            assert!(
                Instant::now() < deadline,
                "connection permit was not returned"
            );
            std::thread::yield_now();
        }
    }

    #[test]
    fn many_connections_reuse_a_bounded_set_of_threads() {
        let workers = ConnectionWorkers::new("reuse-test", 2).unwrap();
        let (done, results) = mpsc::channel();
        let mut threads = HashSet::new();
        for _ in 0..64 {
            let done = done.clone();
            assert!(workers.try_execute(move || done.send(std::thread::current().id()).unwrap()));
            threads.insert(results.recv_timeout(Duration::from_secs(5)).unwrap());
            wait_idle(&workers);
        }
        assert!(threads.len() <= 2);
    }

    #[test]
    fn idle_listener_creates_no_worker_threads() {
        let workers = ConnectionWorkers::new("idle-test", 8).unwrap();
        assert_eq!(*workers.started.lock().unwrap(), 0);
        assert!(workers.try_execute(|| {}));
        wait_idle(&workers);
        assert_eq!(*workers.started.lock().unwrap(), 1);
    }

    #[test]
    fn blocked_connections_do_not_serialize_and_excess_is_rejected() {
        let workers = ConnectionWorkers::new("bound-test", 2).unwrap();
        let gate = Arc::new(std::sync::Barrier::new(3));
        let (started, starts) = mpsc::channel();
        for _ in 0..2 {
            let gate = Arc::clone(&gate);
            let started = started.clone();
            assert!(workers.try_execute(move || {
                started.send(()).unwrap();
                gate.wait();
            }));
        }
        for _ in 0..2 {
            starts.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        assert!(!workers.try_execute(|| panic!("excess connection ran")));
        gate.wait();
        wait_idle(&workers);
        assert!(workers.try_execute(|| {}));
        wait_idle(&workers);
    }

    #[test]
    fn panicking_handler_returns_capacity_and_worker_survives() {
        let workers = ConnectionWorkers::new("panic-test", 1).unwrap();
        assert!(workers.try_execute(|| panic!("connection failed")));
        wait_idle(&workers);
        let (done, result) = mpsc::channel();
        assert!(workers.try_execute(move || done.send(()).unwrap()));
        result.recv_timeout(Duration::from_secs(5)).unwrap();
        wait_idle(&workers);
    }
}
