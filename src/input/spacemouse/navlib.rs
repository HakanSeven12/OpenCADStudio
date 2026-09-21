//! Windows NavLib adapter. Loads the user's installed 3DxWare runtime; neither
//! SDK binaries nor SDK source files are bundled with Open CAD Studio.
//!
//! ABI reference: 3DxWare SDK v4 navlib.h, navlib_types.h, siappcmd_types.h.
//! All callbacks own only a mutex-protected snapshot. NlClose runs before their
//! storage is released. No mutex is held while calling into the SDK.

use super::*;
use iced::futures::{channel::mpsc::Sender, Stream};
use std::ffi::{c_char, c_long, CStr, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

type Get = unsafe extern "C" fn(u64, *const c_char, *mut Value) -> c_long;
type Set = unsafe extern "C" fn(u64, *const c_char, *const Value) -> c_long;
#[repr(C)]
struct Accessor {
    name: *const c_char,
    get: Option<Get>,
    set: Option<Set>,
    param: u64,
}
#[repr(C)]
struct Options {
    size: u32,
    multithreaded: u8,
    flags: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Array {
    data: *const u8,
    len: usize,
}
#[repr(C)]
#[derive(Clone, Copy)]
union Data {
    boolean: u32,
    long: c_long,
    number: f64,
    point: [f64; 3],
    bounds: [f64; 6],
    matrix: [f64; 16],
    array: Array,
    ptr: *const Node,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Value {
    kind: i32,
    data: Data,
}
impl Value {
    fn boolean(v: bool) -> Self {
        Self {
            kind: 1,
            data: Data { boolean: v.into() },
        }
    }
    fn number(v: f64) -> Self {
        Self {
            kind: 4,
            data: Data { number: v },
        }
    }
    fn values(kind: i32, v: &[f64]) -> Self {
        let mut matrix = [0.; 16];
        matrix[..v.len()].copy_from_slice(v);
        Self {
            kind,
            data: Data { matrix },
        }
    }
    fn string(v: &CStr) -> Self {
        Self {
            kind: 13,
            data: Data {
                array: Array {
                    data: v.as_ptr().cast(),
                    len: v.to_bytes().len(),
                },
            },
        }
    }
}

type Create =
    unsafe extern "C" fn(*mut u64, *const c_char, *const Accessor, usize, *const Options) -> c_long;
type Close = unsafe extern "C" fn(u64) -> c_long;
type Read = unsafe extern "C" fn(u64, *const c_char, *mut Value) -> c_long;
type Write = unsafe extern "C" fn(u64, *const c_char, *const Value) -> c_long;
struct Api {
    _library: libloading::Library,
    create: Create,
    close: Close,
    read: Read,
    write: Write,
}
impl Api {
    fn load() -> Result<Self, String> {
        // Search only the Windows system directory, never a drawing's folder.
        let library: libloading::Library = unsafe {
            libloading::os::windows::Library::load_with_flags("TDxNavLib.dll", 0x00000800)
        }
        .map_err(|_| "Install or start 3DxWare, then reconnect your SpaceMouse.".to_owned())?
        .into();
        unsafe {
            Ok(Self {
                create: *library.get(b"NlCreate\0").map_err(|e| e.to_string())?,
                close: *library.get(b"NlClose\0").map_err(|e| e.to_string())?,
                read: *library.get(b"NlReadValue\0").map_err(|e| e.to_string())?,
                write: *library.get(b"NlWriteValue\0").map_err(|e| e.to_string())?,
                _library: library,
            })
        }
    }
    fn write(&self, handle: u64, name: &CStr, value: Value) -> c_long {
        unsafe { (self.write)(handle, name.as_ptr(), &value) }
    }
}

const NO_DATA: c_long = 0x80040078_u32 as c_long;
const INVALID: c_long = 0x80040016_u32 as c_long;

unsafe extern "C" fn get(param: u64, name: *const c_char, out: *mut Value) -> c_long {
    if name.is_null() || out.is_null() {
        return INVALID;
    }
    let name = unsafe { CStr::from_ptr(name) }.to_bytes();
    // NavLib asks for the coordinate system during NlCreate, before any
    // document snapshot or window-focus event is available.
    let constant = match name {
        b"coordinateSystem" => Some(DMat4::from_rotation_x(-std::f64::consts::FRAC_PI_2)),
        b"views.front" => Some(DMat4::from_rotation_x(std::f64::consts::FRAC_PI_2)),
        _ => None,
    };
    if let Some(matrix) = constant {
        unsafe {
            *out = Value::values(7, &matrix.to_cols_array());
        }
        return 0;
    }
    // This Arc's allocation is retained by Connection until NlClose completes.
    let shared = unsafe { &*(param as *const Mutex<Bridge>) };
    let Ok(bridge) = shared.lock() else {
        return NO_DATA;
    };
    let Some(v) = &bridge.view else {
        return NO_DATA;
    };
    let value = match name {
        b"view.affine" => Value::values(7, &v.affine()),
        b"view.extents" => Value::values(11, &v.extents()),
        b"view.perspective" => Value::boolean(v.perspective),
        b"view.rotatable" => Value::boolean(v.rotate),
        b"view.fov" => Value::number(v.fov),
        b"view.focusDistance" => Value::number(v.distance),
        // This SDK property enables its separate target-camera model. OCS's
        // camera.target is only a representation of a free camera's center;
        // advertising it here prevents normal continuous camera navigation.
        // https://forum.3dconnexion.com/viewtopic.php?t=53430
        b"view.target" => return NO_DATA,
        b"view.frustum" => {
            let near = v.distance * 0.001;
            let h = near * (v.fov * 0.5).tan();
            Value::values(
                12,
                &[-h * v.aspect, h * v.aspect, -h, h, near, v.distance * 1000.],
            )
        }
        b"model.extents" => Value::values(11, &v.model_bounds),
        b"model.unitsToMeters" => Value::number(v.units_to_meters),
        b"selection.empty" => Value::boolean(v.selection_bounds.is_none()),
        b"selection.extents" => match v.selection_bounds {
            Some(b) => Value::values(11, &b),
            None => return NO_DATA,
        },
        b"pivot.position" => Value::values(5, &v.pivot.to_array()),
        b"pivot.user" => Value::boolean(v.selection_bounds.is_some()),
        b"pivot.visible" => Value::boolean(v.pivot_visible),
        b"pointer.position" => Value::values(5, &v.pointer.to_array()),
        _ => return NO_DATA,
    };
    unsafe {
        *out = value;
    }
    0
}

unsafe extern "C" fn set(param: u64, name: *const c_char, value: *const Value) -> c_long {
    if name.is_null() || value.is_null() {
        return INVALID;
    }
    let shared = unsafe { &*(param as *const Mutex<Bridge>) };
    let Ok(mut bridge) = shared.lock() else {
        return NO_DATA;
    };
    let name = unsafe { CStr::from_ptr(name) }.to_bytes();
    let value = unsafe { &*value };
    // Profile changes can arrive while the settings window owns focus and
    // there is no drawing snapshot. Query the SDK later, outside this mutex.
    if name == b"settings.changed" && value.kind == 2 {
        bridge.driver_settings_changed = true;
        log::debug!("SpaceMouse: driver settings revision {}", unsafe {
            value.data.long
        });
        if let Some(wake) = &bridge.control {
            wake.signal();
        }
        return 0;
    }
    if name == b"motion" && value.kind == 1 {
        if bridge.view.as_ref().is_some_and(View::planar) {
            return 0;
        }
        bridge.moving = unsafe { value.data.boolean } != 0;
        bridge.wake();
        return 0;
    }
    if name == b"transaction" && value.kind == 2 {
        if bridge.view.as_ref().is_some_and(View::planar) {
            return 0;
        }
        bridge.in_transaction = unsafe { value.data.long } != 0;
        if !bridge.in_transaction {
            bridge.wake();
        }
        return 0;
    }
    if !bridge.focus {
        return NO_DATA;
    }
    if name == b"commands.activeCommand" {
        if !matches!(value.kind, 8 | 13) {
            return INVALID;
        }
        let array = unsafe { value.data.array };
        if array.data.is_null() || array.len > 1024 {
            return INVALID;
        }
        let bytes = unsafe { std::slice::from_raw_parts(array.data, array.len) };
        let Ok(id) = std::str::from_utf8(bytes) else {
            return INVALID;
        };
        let id = id.trim_end_matches('\0');
        if bridge.commands.len() < 32 && bridge.actions.iter().any(|a| a.id == id) {
            if let Some(target) = bridge.view.as_ref().map(|v| v.target.clone()) {
                bridge.commands.push_back((target, id.to_owned()));
                bridge.wake();
            }
        }
        return 0;
    }
    let lock_horizon = bridge.lock_horizon;
    let orbit_about_pivot = bridge.orbit_about_pivot;
    let Some(view) = &mut bridge.view else {
        return NO_DATA;
    };
    if !view.motion_enabled {
        return NO_DATA;
    }
    // Planar navigation combines physical pushes and tilts before NavLib turns them
    // into orbit/zoom camera changes. Buttons still use the SDK connection.
    if view.planar() {
        return 0;
    }
    let accepted = match (name, value.kind) {
        (b"view.affine", 7) => {
            let previous = view.rotation;
            let accepted = view.set_affine(unsafe { value.data.matrix });
            if accepted && lock_horizon {
                let proposed = view.rotation;
                view.lock_horizon(previous, orbit_about_pivot);
                if log::log_enabled!(log::Level::Trace) && (proposed * DVec3::Z).z.abs() > 0.99 {
                    log::trace!(
                        "SpaceMouse: pole previous={previous:?} driver={proposed:?} accepted={:?}",
                        view.rotation
                    );
                }
            }
            accepted
        }
        (b"view.extents", 11) => view.set_extents(unsafe { value.data.bounds }),
        (b"pivot.position", 5) => {
            let p = unsafe { value.data.point };
            let point = DVec3::new(p[0], p[1], p[2]);
            if !point.is_finite() {
                false
            } else {
                view.pivot = point;
                true
            }
        }
        (b"pivot.visible", 1) => {
            view.pivot_visible = unsafe { value.data.boolean } != 0;
            true
        }
        _ => return NO_DATA,
    };
    if !accepted {
        return INVALID;
    }
    bridge.pending_view = bridge.view.clone();
    if !bridge.in_transaction {
        bridge.wake();
    }
    0
}

#[repr(C)]
struct Node {
    size: u32,
    kind: i32,
    next: *const Node,
    children: *const Node,
    id: *const c_char,
    label: *const c_char,
    description: *const c_char,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct ImageData {
    data: *const u8,
    size: usize,
    index: u32,
}
#[repr(C)]
union ImageUnion {
    image: ImageData,
    padding: [usize; 4],
}
#[repr(C)]
struct Image {
    size: u32,
    kind: i32,
    id: *const c_char,
    data: ImageUnion,
}

/// Heap storage survives the entire connection, including deferred SDK reads.
struct Commands {
    _strings: Vec<CString>,
    nodes: Box<[Node]>,
    _pngs: Vec<Vec<u8>>,
    images: Vec<Image>,
}
impl Commands {
    fn new(actions: &[Action]) -> Self {
        let mut strings = vec![
            CString::new("Default").unwrap(),
            CString::new("Open CAD Studio").unwrap(),
            CString::new("").unwrap(),
        ];
        let mut nodes = vec![Node {
            size: std::mem::size_of::<Node>() as u32,
            kind: 0,
            next: std::ptr::null(),
            children: std::ptr::null(),
            id: strings[0].as_ptr(),
            label: strings[1].as_ptr(),
            description: strings[2].as_ptr(),
        }];
        let mut pngs = Vec::new();
        let mut images = Vec::new();
        for action in actions {
            let start = strings.len();
            for text in [&action.id, &action.label, &action.description] {
                strings.push(CString::new(text.replace('\0', "")).unwrap());
            }
            nodes.push(Node {
                size: std::mem::size_of::<Node>() as u32,
                kind: 2,
                next: std::ptr::null(),
                children: std::ptr::null(),
                id: strings[start].as_ptr(),
                label: strings[start + 1].as_ptr(),
                description: strings[start + 2].as_ptr(),
            });
            if let Some(svg) = action.icon {
                if let Some(png) = icon_png(svg) {
                    pngs.push(png);
                    let png = pngs.last().unwrap();
                    images.push(Image {
                        size: std::mem::size_of::<Image>() as u32,
                        kind: 3,
                        id: strings[start].as_ptr(),
                        data: ImageUnion {
                            image: ImageData {
                                data: png.as_ptr(),
                                size: png.len(),
                                index: 0,
                            },
                        },
                    });
                }
            }
        }
        let mut nodes = nodes.into_boxed_slice();
        for i in 1..nodes.len().saturating_sub(1) {
            nodes[i].next = &nodes[i + 1];
        }
        if nodes.len() > 1 {
            nodes[0].children = &nodes[1];
        }
        Self {
            _strings: strings,
            nodes,
            _pngs: pngs,
            images,
        }
    }
}
fn icon_png(svg: &[u8]) -> Option<Vec<u8>> {
    let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(32, 32)?;
    let scale = 32.0 / tree.size().width().max(tree.size().height());
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().ok()
}

struct Connection {
    api: Api,
    handle: u64,
    _accessors: Vec<Accessor>,
    _commands: Commands,
    _shared: Arc<Mutex<Bridge>>,
    policy: Option<(bool, bool, bool, bool)>,
    target: Option<Target>,
}
impl Connection {
    fn open(shared: Arc<Mutex<Bridge>>) -> Result<Self, String> {
        let api = Api::load()?;
        let param = Arc::as_ptr(&shared) as u64;
        let reads: &[&CStr] = &[
            c"coordinateSystem",
            c"view.affine",
            c"view.extents",
            c"view.perspective",
            c"view.rotatable",
            c"view.fov",
            c"view.focusDistance",
            c"view.target",
            c"view.frustum",
            c"views.front",
            c"model.extents",
            c"model.unitsToMeters",
            c"selection.empty",
            c"selection.extents",
            c"pivot.position",
            c"pivot.user",
            c"pivot.visible",
            c"pointer.position",
        ];
        let writes: &[&CStr] = &[
            c"view.affine",
            c"view.extents",
            c"pivot.position",
            c"pivot.visible",
        ];
        let mut accessors: Vec<Accessor> = reads
            .iter()
            .map(|name| Accessor {
                name: name.as_ptr(),
                get: Some(get),
                set: writes.contains(name).then_some(set as Set),
                param,
            })
            .collect();
        for name in [
            c"commands.activeCommand",
            c"transaction",
            c"motion",
            c"settings.changed",
        ] {
            accessors.push(Accessor {
                name: name.as_ptr(),
                get: None,
                set: Some(set),
                param,
            });
        }
        let options = Options {
            size: std::mem::size_of::<Options>() as u32,
            multithreaded: 1,
            flags: 4,
        };
        let commands = Commands::new(&shared.lock().unwrap().actions);
        let mut handle = 0;
        let result = unsafe {
            (api.create)(
                &mut handle,
                c"Open CAD Studio".as_ptr(),
                accessors.as_ptr(),
                accessors.len(),
                &options,
            )
        };
        if result != 0 {
            return Err(format!(
                "3DxWare connection failed ({result:#x}). Start or update 3DxWare."
            ));
        }
        let connection = Self {
            api,
            handle,
            _accessors: accessors,
            _commands: commands,
            _shared: shared,
            policy: None,
            target: None,
        };
        connection.refresh_settings();
        // Iced requests animation frames only while motion is active and
        // supplies their monotonic timestamps. A held puck must keep moving
        // even when it sends no new axis values.
        connection.write_checked(
            c"frame.timingSource",
            Value {
                kind: 2,
                data: Data { long: 1 },
            },
        )?;
        connection.write_checked(
            c"commands.tree",
            Value {
                kind: 9,
                data: Data {
                    ptr: connection._commands.nodes.as_ptr(),
                },
            },
        )?;
        connection.write_checked(c"commands.activeSet", Value::string(c"Default"))?;
        if !connection._commands.images.is_empty() {
            connection.write_checked(
                c"images",
                Value {
                    kind: 14,
                    data: Data {
                        array: Array {
                            data: connection._commands.images.as_ptr().cast(),
                            len: connection._commands.images.len(),
                        },
                    },
                },
            )?;
        }
        Ok(connection)
    }
    fn write_checked(&self, name: &CStr, value: Value) -> Result<(), String> {
        let code = self.api.write(self.handle, name, value);
        if code == 0 {
            Ok(())
        } else {
            Err(format!(
                "3DxWare rejected {} ({code:#x}). Update 3DxWare.",
                name.to_string_lossy()
            ))
        }
    }
    fn present(&self) -> Result<bool, c_long> {
        let mut value = Value::boolean(false);
        let result =
            unsafe { (self.api.read)(self.handle, c"device.present".as_ptr(), &mut value) };
        if result == 0 && value.kind == 1 {
            Ok(unsafe { value.data.boolean } != 0)
        } else {
            Err(result)
        }
    }
    fn string_setting(&self, name: &CStr) -> Option<String> {
        // settings.* properties use caller-owned UTF-8 string storage.
        let mut buffer = [0_u8; 32];
        let mut value = Value {
            kind: 8,
            data: Data {
                array: Array {
                    data: buffer.as_mut_ptr(),
                    len: buffer.len(),
                },
            },
        };
        let result = unsafe { (self.api.read)(self.handle, name.as_ptr(), &mut value) };
        if result != 0 {
            return None;
        }
        let end = buffer.iter().position(|b| *b == 0).unwrap_or(buffer.len());
        Some(std::str::from_utf8(&buffer[..end]).ok()?.trim().to_owned())
    }
    fn bool_setting(&self, name: &CStr) -> Option<bool> {
        match self.string_setting(name)?.to_ascii_lowercase().as_str() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        }
    }
    fn refresh_settings(&self) {
        let value = self.bool_setting(c"settings.LockHorizon");
        let model = self.string_setting(c"settings.MotionModel");
        log::debug!("SpaceMouse: driver Lock Horizon = {value:?}, motion model = {model:?}");
        let mut bridge = self._shared.lock().unwrap();
        if let Some(enabled) = value {
            bridge.lock_horizon = enabled;
        }
        if let Some(model) = model {
            // Object mode rotates about the SDK pivot; Camera/Helicopter modes
            // keep their proposed eye when pitch is constrained.
            bridge.orbit_about_pivot = model == "Examine";
        }
    }
    fn sync(&mut self) {
        let (focus, frame_time, reset_motion, settings_changed, view) = {
            let mut bridge = self._shared.lock().unwrap();
            (
                bridge.focus,
                bridge.frame_time.take(),
                std::mem::take(&mut bridge.reset_motion),
                std::mem::take(&mut bridge.driver_settings_changed),
                bridge.view.clone(),
            )
        };
        let policy = (
            focus,
            view.as_ref().is_some_and(|v| v.rotate),
            view.as_ref().is_some_and(|v| v.zoom),
            view.as_ref().is_some_and(|v| v.motion_enabled),
        );
        let context_changed = match (&self.target, &view) {
            (Some(old), Some(view)) => !old.same_context(&view.target),
            (None, None) => false,
            _ => true,
        };
        if settings_changed || (focus && self.policy.is_none_or(|p| !p.0)) {
            self.refresh_settings();
        }
        if self.policy != Some(policy) || context_changed || reset_motion {
            self.api
                .write(self.handle, c"motion", Value::boolean(false));
            // Native 3DxWare selects the foreground process itself. `focus`
            // is for clients hosted through NLServer; releasing it when the
            // settings UI takes focus can select another application's profile.
            self.api
                .write(self.handle, c"active", Value::boolean(view.is_some()));
            self.api
                .write(self.handle, c"view.rotatable", Value::boolean(policy.1));
            self.policy = Some(policy);
        }
        if let Some(view) = view {
            self.target = Some(view.target);
        } else {
            self.target = None;
        }
        // Do not echo driver-originated camera changes back through Write:
        // those notifications reset NavLib's current navigation model.
        if focus {
            if let Some(time) = frame_time {
                self.api
                    .write(self.handle, c"frame.time", Value::number(time));
            }
        }
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.api
            .write(self.handle, c"active", Value::boolean(false));
        unsafe {
            (self.api.close)(self.handle);
        }
        log::debug!("SpaceMouse: navigation connection closed");
    }
}

struct Stop(
    Arc<AtomicBool>,
    super::worker::Wake,
    Option<std::thread::JoinHandle<()>>,
);
impl Drop for Stop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
        self.1.signal();
        // NlClose persists the driver's per-application settings. Do not leave
        // a detached worker racing process exit after the subscription drops.
        if let Some(thread) = self.2.take() {
            let _ = thread.join();
        }
    }
}

pub(super) fn subscription(service: &Service) -> impl Stream<Item = ()> + use<> {
    let shared = service.0.clone();
    iced::stream::channel(1, move |output: Sender<()>| async move {
        let wake = match super::worker::Wake::new() {
            Ok(wake) => wake,
            Err(error) => {
                publish_status(
                    &shared,
                    Status::Unavailable(format!("SpaceMouse worker: {error}")),
                );
                return;
            }
        };
        let stop = Arc::new(AtomicBool::new(false));
        let mut stop_guard = Stop(stop.clone(), wake.clone(), None);
        let _raw_input = super::raw_input::Listener::start(shared.clone());
        {
            let mut bridge = shared.lock().unwrap();
            bridge.wake = Some(output);
            bridge.control = Some(wake.clone());
        }
        stop_guard.2 = Some(
            std::thread::Builder::new()
                .name("spacemouse".into())
                .spawn(move || {
                    while !stop.load(Ordering::Acquire) {
                        match Connection::open(shared.clone()) {
                            Ok(mut connection) => {
                                let mut last_check = None;
                                loop {
                                    if stop.load(Ordering::Acquire) {
                                        break;
                                    }
                                    connection.sync();
                                    if last_check.is_none_or(|t: std::time::Instant| {
                                        t.elapsed() >= Duration::from_secs(1)
                                    }) {
                                        last_check = Some(std::time::Instant::now());
                                        let status = match connection.present() {
                                            Ok(true) => match super::raw_input::connected() {
                                                Ok(true) => Status::Ready,
                                                Ok(false) => Status::Disconnected,
                                                // Device enumeration can race hotplug. Keep the SDK's
                                                // answer until the next hardware poll in that case.
                                                Err(_) => Status::Ready,
                                            },
                                            Ok(false) => Status::Disconnected,
                                            Err(code) => Status::Unavailable(format!(
                                                "3DxWare stopped responding ({code:#x})."
                                            )),
                                        };
                                        let failed = matches!(status, Status::Unavailable(_));
                                        publish_status(&shared, status);
                                        if failed {
                                            break;
                                        }
                                    }
                                    wake.wait(Duration::from_secs(1));
                                }
                            }
                            Err(reason) => publish_status(&shared, Status::Unavailable(reason)),
                        }
                        if !stop.load(Ordering::Acquire) {
                            wake.wait(Duration::from_secs(5));
                        }
                    }
                })
                .expect("SpaceMouse worker thread"),
        );
        std::future::pending::<()>().await;
    })
}
fn publish_status(shared: &Mutex<Bridge>, status: Status) {
    let mut bridge = shared.lock().unwrap();
    bridge.ever_connected |= status == Status::Ready;
    if bridge.status.as_ref() != Some(&status) {
        if status != Status::Ready {
            bridge.pending_view = None;
            bridge.commands.clear();
            bridge.moving = false;
            bridge.in_transaction = false;
            bridge.pan.clear();
        }
        bridge.status = Some(status);
        bridge.wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sdk_abi_layout() {
        assert_eq!(std::mem::size_of::<Value>(), 136);
        assert_eq!(std::mem::offset_of!(Value, data), 8);
        assert_eq!(std::mem::size_of::<Options>(), 12);
        assert_eq!(std::mem::offset_of!(Options, flags), 8);
        if cfg!(target_pointer_width = "64") {
            assert_eq!(std::mem::size_of::<Accessor>(), 32);
            assert_eq!(std::mem::size_of::<Node>(), 48);
            assert_eq!(std::mem::size_of::<Image>(), 48);
        }
    }
    #[test]
    #[ignore = "requires an installed, running 3DxWare driver"]
    fn installed_driver_connects_and_reports_device_presence() {
        let shared = Arc::new(Mutex::new(Bridge {
            actions: vec![Action {
                id: "UNDO".into(),
                label: "Undo".into(),
                description: "Undo last step".into(),
                icon: Some(crate::ui::window::options::spacemouse::ICON),
            }],
            ..Bridge::default()
        }));
        let connection = Connection::open(shared).unwrap();
        eprintln!(
            "3DxWare connected; device present: {}",
            connection.present().unwrap()
        );
        let horizon = connection.bool_setting(c"settings.LockHorizon");
        eprintln!("3DxWare Lock Horizon: {horizon:?}");
        assert!(
            horizon.is_some(),
            "read Lock Horizon through the installed SDK"
        );
        assert!(connection.string_setting(c"settings.MotionModel").is_some());
    }
    #[test]
    fn settings_changes_are_received_while_the_driver_dialog_has_focus() {
        let service = Service::default();
        let revision = Value {
            kind: 2,
            data: Data { long: 1 },
        };
        assert_eq!(
            unsafe {
                set(
                    Arc::as_ptr(&service.0) as u64,
                    c"settings.changed".as_ptr(),
                    &revision,
                )
            },
            0
        );
        assert!(service.0.lock().unwrap().driver_settings_changed);
    }
    #[test]
    fn orbit_respects_the_driver_horizon_setting_and_unlock_restores_roll() {
        let service = Service::default();
        let mut view = super::super::tests::view();
        view.rotate = true;
        view.zoom = true;
        service.sync(Some(view.clone()), true);
        let param = Arc::as_ptr(&service.0) as u64;
        for locked in [false, true, false] {
            service.0.lock().unwrap().lock_horizon = locked;
            for pitch in [0.4, 1.1, 0., std::f64::consts::PI] {
                let rotation = DQuat::from_rotation_z(0.6)
                    * DQuat::from_rotation_x(pitch)
                    * DQuat::from_rotation_z(0.3);
                let matrix = DMat4::from_rotation_translation(rotation, view.eye);
                assert_eq!(
                    unsafe {
                        set(
                            param,
                            c"view.affine".as_ptr(),
                            &Value::values(7, &matrix.to_cols_array()),
                        )
                    },
                    0
                );
                let result = service.drain().0.unwrap();
                assert!(result.rotation.is_finite());
                let at_pole = pitch == 0. || pitch == std::f64::consts::PI;
                if locked && at_pole {
                    assert!((result.rotation * DVec3::Z).abs_diff_eq(rotation * DVec3::Z, 1.1e-4));
                    assert!(
                        (result.eye.distance(view.pivot) - view.eye.distance(view.pivot)).abs()
                            < 1e-10
                    );
                } else {
                    assert_eq!(result.eye, view.eye);
                    assert!((result.rotation * DVec3::Z).abs_diff_eq(rotation * DVec3::Z, 1e-10));
                }
                if locked {
                    assert!((result.rotation * DVec3::X).z.abs() < 1e-10);
                } else {
                    assert!(result.rotation.abs_diff_eq(rotation, 1e-10));
                }
            }
        }
    }
    #[test]
    fn horizon_locked_orbit_stops_at_both_poles_and_reverses_without_reflection() {
        for perspective in [false, true] {
            for bottom in [false, true] {
                for yaw in [0., 0.7, -2.4] {
                    let service = Service::default();
                    let mut view = super::super::tests::view();
                    view.rotate = true;
                    view.zoom = true;
                    view.perspective = perspective;
                    view.pivot = DVec3::new(13., -27., 5.);
                    let pole = if bottom { std::f64::consts::PI } else { 0. };
                    let inward = if bottom { -1. } else { 1. };
                    let heading = DQuat::from_rotation_z(yaw);
                    view.rotation = heading * DQuat::from_rotation_x(pole + inward * 0.03);
                    view.eye = view.pivot + view.rotation * DVec3::Z * view.distance;
                    service.sync(Some(view.clone()), true);
                    service.0.lock().unwrap().lock_horizon = true;
                    service.0.lock().unwrap().orbit_about_pivot = true;
                    let param = Arc::as_ptr(&service.0) as u64;
                    // Approach, cross, hold beyond, then reverse away from each
                    // pole, including the app camera's f32 quaternion precision.
                    for offset in [0.02, 0.01, 0., -0.01, -0.03, -0.03, -0.03, 0.01, 0.02] {
                        let proposed = heading * DQuat::from_rotation_x(pole + inward * offset);
                        let eye = view.pivot + proposed * DVec3::Z * view.distance;
                        let matrix = DMat4::from_rotation_translation(proposed, eye);
                        assert_eq!(
                            unsafe {
                                set(
                                    param,
                                    c"view.affine".as_ptr(),
                                    &Value::values(7, &matrix.to_cols_array()),
                                )
                            },
                            0
                        );
                        let mut result = service.drain().0.unwrap();
                        result.rotation = result.rotation.as_quat().as_dquat().normalize();
                        let right = result.rotation * DVec3::X;
                        let up = result.rotation * DVec3::Y;
                        let normal = result.rotation * DVec3::Z;
                        assert!(
                            right.dot(heading * DVec3::X) > 0.99999,
                            "screen-right flipped"
                        );
                        assert!(right.z.abs() < 1e-7, "horizon rolled");
                        assert!(up.z >= 0., "camera passed the pole");
                        assert!(
                            (result.eye - normal * result.distance).distance(view.pivot) < 1e-5,
                            "orbit pivot moved on screen"
                        );
                        if offset > 0. {
                            assert!(
                                normal.abs_diff_eq(proposed * DVec3::Z, 1e-7),
                                "cannot reverse away from pole"
                            );
                        } else {
                            assert!(normal.z.abs() > 0.9999999, "pitch should stop at the pole");
                        }
                        service.sync(Some(result), true);
                    }
                }
            }
        }
    }
    #[test]
    fn horizon_locked_camera_mode_preserves_the_eye_at_a_pole() {
        let service = Service::default();
        let mut view = super::super::tests::view();
        view.rotate = true;
        view.zoom = true;
        view.perspective = true;
        view.rotation = DQuat::from_rotation_x(0.01);
        service.sync(Some(view.clone()), true);
        service.0.lock().unwrap().lock_horizon = true;
        let param = Arc::as_ptr(&service.0) as u64;
        for tilt in [-0.01, -0.02, 0.01] {
            let rotation = DQuat::from_rotation_x(tilt);
            let eye = view.eye + DVec3::new(1., 2., 3.);
            let matrix = DMat4::from_rotation_translation(rotation, eye);
            assert_eq!(
                unsafe {
                    set(
                        param,
                        c"view.affine".as_ptr(),
                        &Value::values(7, &matrix.to_cols_array()),
                    )
                },
                0
            );
            let result = service.drain().0.unwrap();
            assert_eq!(
                result.eye, eye,
                "pitch limits must preserve camera-mode translation"
            );
            assert!((result.rotation * DVec3::Y).z > 0.);
        }
    }
    #[test]
    fn horizon_lock_allows_yaw_around_the_full_circle() {
        let service = Service::default();
        let mut view = super::super::tests::view();
        view.rotate = true;
        view.zoom = true;
        view.rotation = DQuat::from_rotation_x(0.01);
        service.sync(Some(view.clone()), true);
        service.0.lock().unwrap().lock_horizon = true;
        let param = Arc::as_ptr(&service.0) as u64;
        // A large yaw change is not a pitch through the pole either.
        for yaw in (0..=72).map(|i| i as f64 * 0.1).chain([10., 13.]) {
            let rotation = DQuat::from_rotation_z(yaw) * DQuat::from_rotation_x(0.01);
            let matrix = DMat4::from_rotation_translation(rotation, view.eye);
            assert_eq!(
                unsafe {
                    set(
                        param,
                        c"view.affine".as_ptr(),
                        &Value::values(7, &matrix.to_cols_array()),
                    )
                },
                0
            );
            let result = service.drain().0.unwrap();
            assert!(result.rotation.dot(rotation).abs() > 1. - 1e-10);
            assert_eq!(result.eye, view.eye);
        }
    }
    #[test]
    fn sdk_motion_cannot_double_apply_planar_push_tilt_or_zoom() {
        for zoom in [false, true] {
            let service = Service::default();
            let mut view = super::super::tests::view();
            view.zoom = zoom;
            service.sync(Some(view), true);
            let param = Arc::as_ptr(&service.0) as u64;
            for (name, value) in [
                (c"motion", Value::boolean(true)),
                (
                    c"view.affine",
                    Value::values(7, &DMat4::IDENTITY.to_cols_array()),
                ),
                (
                    c"view.extents",
                    Value::values(11, &[-2., -2., -2., 2., 2., 2.]),
                ),
            ] {
                assert_eq!(unsafe { set(param, name.as_ptr(), &value) }, 0);
            }
            assert!(!service.moving());
            assert!(service.drain().0.is_none());
        }
    }
    #[test]
    fn coordinate_system_is_available_before_a_drawing_exists() {
        let bridge = Mutex::new(Bridge::default());
        let mut value = Value::boolean(false);
        assert_eq!(
            unsafe {
                get(
                    &bridge as *const _ as u64,
                    c"coordinateSystem".as_ptr(),
                    &mut value,
                )
            },
            0
        );
        let matrix = DMat4::from_cols_array(&unsafe { value.data.matrix });
        assert!((matrix.transform_vector3(DVec3::Z) - DVec3::Y).length() < 1e-10);
    }
    #[test]
    fn free_camera_does_not_advertise_the_sdk_target_camera_model() {
        let service = Service::default();
        service.sync(Some(super::super::tests::view()), true);
        let mut value = Value::values(5, &[0., 0., 0.]);
        assert_eq!(
            unsafe {
                get(
                    Arc::as_ptr(&service.0) as u64,
                    c"view.target".as_ptr(),
                    &mut value,
                )
            },
            NO_DATA
        );
    }
    #[test]
    fn paused_motion_is_rejected_but_resume_action_is_queued() {
        let service = Service::default();
        let mut view = super::super::tests::view();
        view.motion_enabled = false;
        service.sync(Some(view), true);
        service.set_actions(vec![Action {
            id: "SPACEMOUSEPAUSE".into(),
            label: "Resume".into(),
            description: String::new(),
            icon: None,
        }]);
        let param = Arc::as_ptr(&service.0) as u64;
        assert_eq!(
            unsafe {
                set(
                    param,
                    c"view.affine".as_ptr(),
                    &Value::values(7, &DMat4::IDENTITY.to_cols_array()),
                )
            },
            NO_DATA
        );
        assert_eq!(
            unsafe {
                set(
                    param,
                    c"commands.activeCommand".as_ptr(),
                    &Value::string(c"SPACEMOUSEPAUSE"),
                )
            },
            0
        );
        let (view, commands) = service.drain();
        assert!(view.is_none());
        assert_eq!(commands[0].1, "SPACEMOUSEPAUSE");
        service.sync(None, false);
        assert_eq!(
            unsafe {
                set(
                    param,
                    c"commands.activeCommand".as_ptr(),
                    &Value::string(c"SPACEMOUSEPAUSE"),
                )
            },
            NO_DATA
        );
        assert!(service.drain().1.is_empty());
    }
}
