//! Linux adapter: reads the spacenavd daemon socket (the same daemon Blender
//! and FreeCAD use). Speaks the daemon's version-0 protocol: the client sends
//! nothing, and every event is eight native-endian i32 values.
//!
//! With its default configuration spacenavd forwards the device axes
//! unchanged (evdev ABS_X..ABS_RZ in HID order), so a motion sample feeds the
//! same push/tilt/lift mapping as the Windows HID reader.

use super::*;
use iced::futures::{channel::mpsc::Sender, Stream};
use std::io::{ErrorKind, Read};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const MOTION: i32 = 0;
const PRESS: i32 = 1;

/// Default button actions; spacenavd has no per-application button editor.
const BUTTONS: [&str; 2] = ["SPACEMOUSEFIT", "SPACEMOUSETOP"];

/// libspnav's lookup order, plus the modern `/run` default.
fn socket_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = std::env::var_os("SPNAV_SOCKET") {
        paths.push(path.into());
    }
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        paths.push(PathBuf::from(dir).join("spnav.sock"));
    }
    paths.push("/run/spnav.sock".into());
    paths.push("/var/run/spnav.sock".into());
    paths
}

fn connect() -> Result<UnixStream, String> {
    for path in socket_paths() {
        if let Ok(stream) = UnixStream::connect(&path) {
            return Ok(stream);
        }
    }
    Err("spacenavd is not running. Install and start spacenavd; Open CAD Studio connects automatically.".into())
}

/// Apply one daemon event to the bridge.
fn event(bridge: &mut Bridge, data: [i32; 8]) {
    match data[0] {
        MOTION => {
            if !bridge.focus || !bridge.view.as_ref().is_some_and(|v| v.motion_enabled) {
                return;
            }
            let zoom = bridge.view.as_ref().is_some_and(|v| v.zoom);
            let was_moving = bridge.pan.moving(zoom, true);
            let axes = std::array::from_fn(|i| data[1 + i].clamp(-32768, 32767) as i16);
            bridge.pan.set_axes(axes, true);
            if was_moving || bridge.pan.moving(zoom, true) {
                bridge.wake();
            }
        }
        PRESS => {
            let Some(target) = bridge.view.as_ref().map(|v| v.target.clone()) else {
                return;
            };
            if let Some(command) = usize::try_from(data[1]).ok().and_then(|b| BUTTONS.get(b)) {
                bridge.commands.push_back((target, (*command).to_string()));
                bridge.wake();
            }
        }
        _ => {}
    }
}

struct Stop(Arc<AtomicBool>, Option<std::thread::JoinHandle<()>>);
impl Drop for Stop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
        if let Some(thread) = self.1.take() {
            let _ = thread.join();
        }
    }
}

pub(super) fn subscription(service: &Service) -> impl Stream<Item = ()> + use<> {
    let shared = service.0.clone();
    iced::stream::channel(1, move |output: Sender<()>| async move {
        shared.lock().unwrap().wake = Some(output);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let _stop = Stop(
            stop,
            std::thread::Builder::new()
                .name("spacemouse".into())
                .spawn(move || run(&shared, &worker_stop))
                .ok(),
        );
        std::future::pending::<()>().await;
    })
}

fn run(shared: &Mutex<Bridge>, stop: &AtomicBool) {
    while !stop.load(Ordering::Acquire) {
        match connect() {
            Ok(mut stream) => {
                // The timeout only lets the loop notice an app shutdown.
                let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                publish_status(shared, Status::Ready);
                let mut buffer = [0_u8; 32];
                let mut filled = 0;
                while !stop.load(Ordering::Acquire) {
                    match stream.read(&mut buffer[filled..]) {
                        Ok(0) => break,
                        Ok(n) => {
                            filled += n;
                            if filled == buffer.len() {
                                filled = 0;
                                let data = std::array::from_fn(|i| {
                                    let bytes = buffer[i * 4..i * 4 + 4].try_into().unwrap();
                                    i32::from_ne_bytes(bytes)
                                });
                                event(&mut shared.lock().unwrap(), data);
                            }
                        }
                        Err(e)
                            if matches!(
                                e.kind(),
                                ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                            ) => {}
                        Err(_) => break,
                    }
                }
                publish_status(shared, Status::Disconnected);
            }
            Err(reason) => publish_status(shared, Status::Unavailable(reason)),
        }
        for _ in 0..10 {
            if stop.load(Ordering::Acquire) {
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }
}

