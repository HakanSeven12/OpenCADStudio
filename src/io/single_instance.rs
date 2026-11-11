//! Single instance: a double-clicked drawing opens as a tab in the editor that
//! is already running, instead of starting a second one.
//!
//! The election *is* the `bind`. Every GUI launch tries to bind a deterministic
//! per-user loopback port; the OS grants it to exactly one process and releases
//! it on any death, `SIGKILL` included. That deletes the whole staleness
//! category a lock file would carry — no PID to probe for liveness, no PID
//! recycling, no inode to unlink, nothing to reap after a crash.
//!
//! Whoever binds keeps the listener and serves it from an iced subscription.
//! Whoever fails to bind connects, checks it is talking to a matching editor,
//! hands the path over, and exits. Every surprise — no answer, a stranger on
//! the port, a timeout — falls back to booting a normal window, which is the
//! behaviour from before this module existed. The feature can degrade, but it
//! cannot lose the file.
//!
//! This port speaks exactly two ops, `ping` and `open`. The automation server
//! in [`crate::app::automation`] is deliberately unreachable from here: it
//! dispatches arbitrary commands (`run`) and writes arbitrary paths (`save`),
//! and its own `open` replaces the active tab's document in place — which would
//! discard the user's unsaved drawing. We reuse that server's line-delimited
//! JSON framing and nothing else.
//!
//! Loopback is reachable by every local process and account, so both ends
//! prove themselves with a random secret the primary writes to a file only its
//! own user can read ([`secret`]): the client sends one half, the editor
//! answers with the other, and `open` is refused on a connection that has not
//! proved the first half.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
#[cfg(target_os = "macos")]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};

/// Protocol tag. Bump the suffix on any wire-format change so a running older
/// editor is recognised as a stranger and both processes degrade cleanly
/// instead of misreading each other.
const MAGIC: &str = "OpenCADStudio/si/3";

/// Neither end blocks forever. Long enough to cover a busy primary's accept
/// backlog, short enough that a wedged peer costs a visible pause and not a
/// hang.
const IO_TIMEOUT: Duration = Duration::from_secs(2);

/// The bound listener, parked between [`claim`] (which runs in `main`, before
/// iced exists) and [`subscribe`] (which runs inside the iced runtime).
/// A static is unavoidable: [`iced::Subscription::run`] takes a plain
/// `fn() -> S`, so the stream builder cannot capture anything.
static LISTENER: Mutex<Option<TcpListener>> = Mutex::new(None);

/// This editor's secret, created when it wins the port.
static SECRET: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

fn secret_path() -> Option<PathBuf> {
    Some(
        crate::config::config_dir()?
            .join("single-instance")
            .join(format!("{}.key", port_for_user())),
    )
}

/// The primary's secret: 64 hex digits, written to a file only this user can
/// read. The first half is the client's proof, the second the editor's.
/// `None` when the file cannot be written; handoffs then fail and every launch
/// boots its own window.
fn secret() -> Option<&'static str> {
    SECRET
        .get_or_init(|| {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).ok()?;
            let secret: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            let path = secret_path()?;
            let dir = path.parent()?;
            std::fs::create_dir_all(dir).ok()?;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
                std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).ok()?;
                options.mode(0o600);
            }
            let mut file = options.open(&path).ok()?;
            // A file left by an older editor may predate the mode above.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(std::fs::Permissions::from_mode(0o600))
                    .ok()?;
            }
            file.write_all(secret.as_bytes()).ok()?;
            Some(secret)
        })
        .as_deref()
}

/// The running editor's secret as its file holds it, split into the client's
/// and the editor's proof.
fn read_secret() -> Option<(String, String)> {
    let text = std::fs::read_to_string(secret_path()?).ok()?;
    let text = text.trim();
    (text.len() == 64).then(|| (text[..32].to_string(), text[32..].to_string()))
}

/// Who we turned out to be.
pub enum Claim {
    /// We own the port. Keep booting; [`subscribe`] will serve it.
    Primary,
    /// Someone else owns it — an editor, or a stranger. Connected stream.
    Existing(TcpStream),
}

/// Use the app bundle path on macOS so its launcher and GUI share a port.
/// Development builds and other platforms remain scoped by executable path.
fn rendezvous_anchor() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    #[cfg(target_os = "macos")]
    {
        // <bundle>.app/Contents/MacOS/<executable> -> .../MacOS -> .../Contents -> <bundle>.app
        if let Some(bundle) = exe
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("app"))
        {
            return bundle.to_path_buf();
        }
    }
    exe
}

/// What makes two launches "the same editor, for the same user, right here".
///
/// All three parts are load-bearing:
///   * user — loopback is shared across accounts on one machine, so without it
///     one user's drawing would surface on another user's screen;
///   * session — the same user on two seats (or an SSH-forwarded display) must
///     not have files delivered to the other display;
///   * rendezvous anchor — otherwise `cargo run` silently hands your test file
///     to an installed copy, which makes this feature hostile to maintain.
fn rendezvous_key() -> String {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default();
    let session = std::env::var("XDG_SESSION_ID")
        .or_else(|_| std::env::var("WAYLAND_DISPLAY"))
        .or_else(|_| std::env::var("DISPLAY"))
        .unwrap_or_default();
    let anchor = rendezvous_anchor().to_string_lossy().into_owned();
    format!("{user}|{session}|{anchor}")
}

/// Deterministic port for this rendezvous key (FNV-1a, folded into a fixed
/// window).
///
/// 29000..31000 sits below every target's ephemeral range (Linux 32768+,
/// Windows/macOS 49152+), so the OS never hands our port to a transient socket
/// while the editor is down. A stranger squatting it is still possible; that
/// costs one [`IO_TIMEOUT`] pause and then a normal boot — a considered trade,
/// not a magic number.
fn port_for_user() -> u16 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in rendezvous_key().bytes().chain(MAGIC.bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    29000 + (h % 2000) as u16
}

fn addr() -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, port_for_user()))
}

/// Try to become the editor that serves this user's double-clicks.
///
/// Binds loopback only — never `0.0.0.0`, which would raise a firewall prompt
/// on Windows and expose the port to the network.
pub fn claim() -> Claim {
    match TcpListener::bind(addr()) {
        Ok(l) => {
            *LISTENER.lock().unwrap_or_else(|e| e.into_inner()) = Some(l);
            secret();
            Claim::Primary
        }
        Err(_) => match try_connect_existing() {
            Some(s) => Claim::Existing(s),
            // Bound a moment ago, gone now: the holder exited between our bind
            // and our connect. We hold no listener, so this window cannot serve
            // — the next launch binds properly. Self-healing.
            None => Claim::Primary,
        },
    }
}

/// Reach an editor without claiming its port. Used by the macOS launcher.
pub fn try_connect_existing() -> Option<TcpStream> {
    TcpStream::connect_timeout(&addr(), IO_TIMEOUT).ok()
}

/// Hand `paths` to the editor on the other end. `true` once it has acknowledged.
///
/// Takes the whole selection in one message: `%F` in the desktop entry hands
/// every double-clicked drawing to a single launch, and forwarding them
/// together keeps them one unit rather than a race between connections.
///
/// Pings first with the client half of the editor's secret and only discloses
/// the paths to a peer that answers with our own [`MAGIC`] and the editor half,
/// so a stranger — or a hash collision — learns nothing about what the user is
/// opening.
pub fn handoff(stream: TcpStream, paths: &[PathBuf]) -> bool {
    let Some((client_proof, editor_proof)) = read_secret() else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let Ok(write_half) = stream.try_clone() else {
        return false;
    };
    let mut w = write_half;
    let mut r = BufReader::new(stream);

    let ping = json!({ "op": "ping", "proof": client_proof });
    if writeln!(w, "{ping}").is_err() || w.flush().is_err() {
        return false;
    }
    let mut line = String::new();
    if r.read_line(&mut line).is_err() {
        return false;
    }
    let ack: Value = match serde_json::from_str(&line) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if ack["app"].as_str() != Some(MAGIC) || ack["proof"].as_str() != Some(editor_proof.as_str()) {
        return false;
    }

    // Absolute, not canonical: the editor's working directory differs from
    // ours, so a relative argument must be resolved here — but `canonicalize`
    // would demand the file exist (we want the editor's own error message, not
    // a silent boot) and on Windows yields a `\\?\` verbatim path that would
    // land verbatim in the recents list.
    let abs: Vec<String> = paths
        .iter()
        .map(|p| {
            std::path::absolute(p)
                .unwrap_or_else(|_| p.clone())
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    let req = json!({ "op": "open", "paths": abs });
    if writeln!(w, "{req}").is_err() || w.flush().is_err() {
        return false;
    }
    line.clear();
    if r.read_line(&mut line).is_err() {
        return false;
    }
    let done: Value = match serde_json::from_str(&line) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if done["ok"].as_bool() != Some(true) {
        return false;
    }

    // We are the foreground process — the file manager just launched us — so we
    // hold the right to hand that privilege to the editor, which does not.
    // Without this the drawing opens in a window that stays behind whatever the
    // user was looking at, and the double-click reads as "nothing happened".
    #[cfg(windows)]
    if let Some(pid) = ack["pid"].as_u64() {
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(pid as u32);
        }
    }
    true
}

/// Serve the claimed port: every accepted `open` yields its path.
///
/// Inert unless [`claim`] returned [`Claim::Primary`] in this process, so a
/// window that lost the election simply never produces items.
pub fn subscribe() -> iced::Subscription<PathBuf> {
    // `worker` must stay a plain `fn` — `Subscription::run` keys the
    // subscription's identity off the function pointer, so turning this into a
    // closure would silently stop the listener with no error.
    iced::Subscription::run(worker)
}

type PathSender = iced::futures::channel::mpsc::Sender<PathBuf>;

fn worker() -> impl iced::futures::Stream<Item = PathBuf> {
    iced::stream::channel(8, serve_claimed_port)
}

async fn serve_claimed_port(out: PathSender) {
    let listener = LISTENER.lock().unwrap_or_else(|e| e.into_inner()).take();
    let Some(listener) = listener else {
        // Not the primary: park forever rather than end the stream, so iced
        // does not re-run the recipe.
        std::future::pending::<()>().await;
        return;
    };
    // `accept` blocks, so it lives on its own OS thread and only ever reaches
    // the iced runtime through the non-blocking `try_send`.
    std::thread::spawn(move || {
        let mut out = out;
        for stream in listener.incoming().flatten() {
            serve_one(&mut out, stream);
        }
    });
    std::future::pending::<()>().await;
}

/// One connection: a ping that proves the client half of the secret, then at
/// most one open.
fn serve_one(out: &mut PathSender, stream: TcpStream) {
    let Some(secret) = secret() else {
        return;
    };
    let (client_proof, editor_proof) = secret.split_at(32);
    let mut proved = false;
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let Ok(write_half) = stream.try_clone() else {
        return;
    };
    let mut w = write_half;
    let mut r = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        if !r.read_line(&mut line).map(|n| n > 0).unwrap_or(false) {
            return;
        }
        let Ok(req) = serde_json::from_str::<Value>(&line) else {
            return;
        };
        match req["op"].as_str() {
            Some("ping") => {
                if req["proof"].as_str() != Some(client_proof) {
                    return;
                }
                proved = true;
                let ack = json!({
                    "ok": true,
                    "app": MAGIC,
                    "proof": editor_proof,
                    "pid": std::process::id(),
                });
                if writeln!(w, "{ack}").is_err() || w.flush().is_err() {
                    return;
                }
            }
            Some("open") if proved => {
                // All-or-nothing: a partial send would silently drop drawings
                // the user selected, which is the failure this whole path
                // exists to avoid.
                let ok = match req["paths"].as_array() {
                    Some(a) => a
                        .iter()
                        .filter_map(|p| p.as_str())
                        .all(|p| out.try_send(PathBuf::from(p)).is_ok()),
                    None => false,
                };
                let _ = writeln!(w, "{}", json!({ "ok": ok }));
                let _ = w.flush();
                return;
            }
            _ => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_is_deterministic_and_in_the_reserved_window() {
        let a = port_for_user();
        let b = port_for_user();
        assert_eq!(a, b, "same process must always compute the same port");
        assert!(
            (29000..31000).contains(&a),
            "port {a} escaped the reserved window"
        );
    }

    #[test]
    fn rendezvous_key_carries_user_session_and_anchor() {
        // All three parts must be present, or the isolation the key exists for
        // is silently gone.
        let k = rendezvous_key();
        assert_eq!(k.matches('|').count(), 2, "key shape changed: {k:?}");
        let anchor = rendezvous_anchor();
        assert!(
            k.ends_with(&*anchor.to_string_lossy()),
            "key must pin the rendezvous anchor: {k:?}"
        );
    }

    #[test]
    fn claim_elects_exactly_one_owner() {
        // Hold the port the way a primary would, then prove a second claim in
        // this process does not also think it is primary.
        let held = match TcpListener::bind(addr()) {
            Ok(l) => l,
            // Another OCS (or a stray test binary) already owns it — the very
            // condition under test cannot be set up, so skip rather than lie.
            Err(_) => return,
        };
        match claim() {
            Claim::Existing(_) => {}
            Claim::Primary => {
                panic!("bind was already held; claim() must not elect a second owner")
            }
        }
        drop(held);
    }

    #[test]
    fn handoff_refuses_a_stranger_on_the_port() {
        // A squatter that answers something other than our ack must never be
        // told which file the user is opening.
        let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) {
            Ok(l) => l,
            Err(_) => return,
        };
        let port = listener.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            if let Ok((s, _)) = listener.accept() {
                let mut w = s.try_clone().unwrap();
                let mut r = BufReader::new(s);
                let mut line = String::new();
                let _ = r.read_line(&mut line);
                // Wrong app tag — a different program that happens to be here.
                let _ = writeln!(w, "{}", json!({ "ok": true, "app": "something-else" }));
                let _ = w.flush();
                // Read anything more: if handoff leaked the path, this sees it.
                line.clear();
                let _ = r.read_line(&mut line);
                line
            } else {
                String::new()
            }
        });
        let s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        assert!(
            !handoff(s, &[PathBuf::from("/tmp/secret.dwg")]),
            "handoff must reject a peer that fails the identity check"
        );
        let leaked = t.join().unwrap();
        assert!(
            !leaked.contains("secret.dwg"),
            "path disclosed to a stranger: {leaked:?}"
        );
    }

    #[test]
    fn the_editor_refuses_a_client_without_the_secret() {
        let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) {
            Ok(l) => l,
            Err(_) => return,
        };
        let port = listener.local_addr().unwrap().port();
        let (tx, mut rx) = iced::futures::channel::mpsc::channel::<PathBuf>(8);
        std::thread::spawn(move || {
            let mut out = tx;
            for stream in listener.incoming().flatten() {
                serve_one(&mut out, stream);
            }
        });
        for request in [
            vec![json!({ "op": "open", "paths": ["/tmp/forced.dwg"] })],
            vec![
                json!({ "op": "ping", "proof": "0".repeat(32) }),
                json!({ "op": "open", "paths": ["/tmp/forced.dwg"] }),
            ],
        ] {
            let s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
            let mut w = s.try_clone().unwrap();
            for line in &request {
                let _ = writeln!(w, "{line}");
            }
            let mut reply = String::new();
            let _ = BufReader::new(s).read_line(&mut reply);
            assert!(!reply.contains("proof"), "editor proof disclosed: {reply:?}");
        }
        assert!(rx.try_recv().is_err(), "a path was opened without the secret");
    }

    #[test]
    fn every_one_of_three_concurrent_handoffs_arrives() {
        // Selecting three drawings launches three processes at once (measured:
        // `%f` + 3 files = 3 spawns, one file each). All three must land.
        let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) {
            Ok(l) => l,
            Err(_) => return,
        };
        let port = listener.local_addr().unwrap().port();
        let (tx, mut rx) = iced::futures::channel::mpsc::channel::<PathBuf>(8);
        // The secret file exists before the first client reads it, as it
        // does once `claim` has elected the editor.
        assert!(secret().is_some());
        // The real accept loop, verbatim.
        std::thread::spawn(move || {
            let mut out = tx;
            for stream in listener.incoming().flatten() {
                serve_one(&mut out, stream);
            }
        });

        let senders: Vec<_> = (0..3)
            .map(|i| {
                std::thread::spawn(move || {
                    let s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
                    handoff(s, &[PathBuf::from(format!("/tmp/ocs_concurrent_{i}.dwg"))])
                })
            })
            .collect();
        for (i, t) in senders.into_iter().enumerate() {
            assert!(t.join().unwrap(), "handoff {i} reported failure");
        }

        let mut got = Vec::new();
        while let Ok(p) = rx.try_recv() {
            got.push(p.to_string_lossy().into_owned());
        }
        got.sort();
        assert_eq!(got.len(), 3, "expected all three paths, got {got:?}");
    }

    #[test]
    fn handoff_gives_up_on_a_peer_that_never_answers() {
        // A wedged peer must cost a timeout, not a hang.
        let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) {
            Ok(l) => l,
            Err(_) => return,
        };
        let port = listener.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            let _held = listener.accept();
            std::thread::sleep(IO_TIMEOUT * 3);
        });
        let s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        let start = iced::time::Instant::now();
        assert!(!handoff(s, &[PathBuf::from("/tmp/a.dwg")]));
        assert!(
            start.elapsed() < IO_TIMEOUT * 2,
            "handoff blocked for {:?} — the read timeout is not applied",
            start.elapsed()
        );
        drop(t);
    }
}
