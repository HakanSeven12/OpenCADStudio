//! Windows multi-axis HID receiver for push-or-tilt planar navigation.
//! Registers only Generic Desktop / Multi-axis Controller, never a keyboard
//! or ordinary mouse. The driver continues to own buttons and 3D navigation.
use super::*;
use std::sync::atomic::{AtomicIsize, Ordering};
use windows_sys::Win32::{
    Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{Input::*, WindowsAndMessaging::*},
};

pub(super) struct Listener {
    window: Arc<AtomicIsize>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Listener {
    pub fn start(shared: Arc<Mutex<Bridge>>) -> Self {
        let window = Arc::new(AtomicIsize::new(0));
        let worker_window = window.clone();
        let thread = std::thread::Builder::new()
            .name("spacemouse-pan".into())
            .spawn(move || {
                if let Err(error) = run(shared, &worker_window) {
                    eprintln!("SpaceMouse pan input: {error}");
                }
            })
            .ok();
        Self { window, thread }
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        let hwnd = self.window.swap(-1, Ordering::AcqRel);
        if hwnd > 0 {
            unsafe {
                PostMessageW(hwnd as HWND, WM_CLOSE, 0, 0);
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(shared: Arc<Mutex<Bridge>>, window: &AtomicIsize) -> Result<(), String> {
    let class: Vec<u16> = "OpenCADStudio.SpaceMouse.Pan\0".encode_utf16().collect();
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let definition = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..Default::default()
        };
        // Another subscription may have registered the class earlier.
        if RegisterClassW(&definition) == 0 {
            let mut existing = WNDCLASSW::default();
            if GetClassInfoW(instance, class.as_ptr(), &mut existing) == 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
        }
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            std::ptr::null(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Arc::as_ptr(&shared) as isize);
        if window
            .compare_exchange(0, hwnd as isize, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            DestroyWindow(hwnd);
            return Ok(());
        }
        let device = RAWINPUTDEVICE {
            usUsagePage: 1,
            usUsage: 8,
            dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
            hwndTarget: hwnd,
        };
        if RegisterRawInputDevices(&device, 1, std::mem::size_of::<RAWINPUTDEVICE>() as u32) == 0 {
            window.store(0, Ordering::Release);
            DestroyWindow(hwnd);
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut message = MSG::default();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        let remove = RAWINPUTDEVICE {
            dwFlags: RIDEV_REMOVE,
            hwndTarget: std::ptr::null_mut(),
            ..device
        };
        RegisterRawInputDevices(&remove, 1, std::mem::size_of::<RAWINPUTDEVICE>() as u32);
        window.store(0, Ordering::Release);
    }
    Ok(())
}

unsafe extern "system" fn procedure(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if message == WM_DESTROY {
        unsafe {
            PostQuitMessage(0);
        }
        return 0;
    }
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Mutex<Bridge>;
    if !pointer.is_null() {
        let shared = unsafe { &*pointer };
        if message == WM_INPUT {
            input(shared, lp as HRAWINPUT);
        } else if message == WM_INPUT_DEVICE_CHANGE && wp == GIDC_REMOVAL as usize {
            if let Ok(mut bridge) = shared.lock() {
                bridge.pan.clear();
                bridge.wake();
            }
        }
    }
    unsafe { DefWindowProcW(hwnd, message, wp, lp) }
}

fn is_spacemouse(device: HANDLE) -> bool {
    let mut info = RID_DEVICE_INFO {
        cbSize: std::mem::size_of::<RID_DEVICE_INFO>() as u32,
        ..Default::default()
    };
    let mut size = info.cbSize;
    unsafe {
        if GetRawInputDeviceInfoW(
            device,
            RIDI_DEVICEINFO,
            (&mut info as *mut RID_DEVICE_INFO).cast(),
            &mut size,
        ) == u32::MAX
            || info.dwType != RIM_TYPEHID
        {
            return false;
        }
        let hid = info.Anonymous.hid;
        // Logitech's legacy SpaceMouse IDs and current 3Dconnexion vendor ID.
        hid.usUsagePage == 1
            && hid.usUsage == 8
            && (hid.dwVendorId == 0x256f
                || (hid.dwVendorId == 0x046d && (0xc603..=0xc631).contains(&hid.dwProductId)))
    }
}

/// Older NavLib versions report device.present as always true. Enumerate the
/// registered multi-axis HID devices as well, so availability reflects hardware.
pub(super) fn connected() -> Result<bool, String> {
    let element_size = std::mem::size_of::<RAWINPUTDEVICELIST>() as u32;
    let mut count = 0;
    unsafe {
        if GetRawInputDeviceList(std::ptr::null_mut(), &mut count, element_size) == u32::MAX {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut devices = vec![RAWINPUTDEVICELIST::default(); count as usize];
        let read = GetRawInputDeviceList(devices.as_mut_ptr(), &mut count, element_size);
        if read == u32::MAX {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(devices
            .iter()
            .take(read as usize)
            .any(|device| device.dwType == RIM_TYPEHID && is_spacemouse(device.hDevice)))
    }
}

fn input(shared: &Mutex<Bridge>, handle: HRAWINPUT) {
    // u64 storage provides the alignment required by RAWINPUTHEADER.
    let mut buffer = [0_u64; 128];
    let mut size = std::mem::size_of_val(&buffer) as u32;
    let header_size = std::mem::size_of::<RAWINPUTHEADER>();
    let read = unsafe {
        GetRawInputData(
            handle,
            RID_INPUT,
            buffer.as_mut_ptr().cast(),
            &mut size,
            header_size as u32,
        )
    };
    if read == u32::MAX
        || read < (header_size + 8) as u32
        || read as usize > std::mem::size_of_val(&buffer)
    {
        return;
    }
    let header = unsafe { &*buffer.as_ptr().cast::<RAWINPUTHEADER>() };
    if header.dwType != RIM_TYPEHID || !is_spacemouse(header.hDevice) {
        return;
    }
    let bytes = unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), read as usize) };
    let width =
        u32::from_le_bytes(bytes[header_size..header_size + 4].try_into().unwrap()) as usize;
    let count =
        u32::from_le_bytes(bytes[header_size + 4..header_size + 8].try_into().unwrap()) as usize;
    if width == 0 || count > (bytes.len() - header_size - 8) / width {
        return;
    }
    let Ok(mut bridge) = shared.lock() else {
        return;
    };
    if !bridge.focus
        || !bridge
            .view
            .as_ref()
            .is_some_and(|v| v.planar() && v.motion_enabled)
    {
        return;
    }
    let zoom = bridge.view.as_ref().unwrap().zoom;
    let was_moving = bridge.pan.moving(zoom);
    let mut accepted = false;
    for report in bytes[header_size + 8..].chunks_exact(width).take(count) {
        accepted |= bridge.pan.report(report);
    }
    if accepted && (was_moving || bridge.pan.moving(zoom)) {
        bridge.wake();
    }
}
