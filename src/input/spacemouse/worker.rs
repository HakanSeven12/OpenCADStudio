//! Wait for app work and dispatch messages to windows owned by the SDK thread.
//! A channel-only wait leaves Windows-delivered driver settings unprocessed.
use std::{sync::Arc, time::Duration};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::Threading::{CreateEventW, SetEvent},
    UI::WindowsAndMessaging::*,
};

struct Event(isize);
impl Drop for Event {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0 as HANDLE);
        }
    }
}

#[derive(Clone)]
pub(super) struct Wake(Arc<Event>);
impl Wake {
    pub fn new() -> Result<Self, std::io::Error> {
        let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
        if event.is_null() {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(Self(Arc::new(Event(event as isize))))
        }
    }
    pub fn signal(&self) {
        unsafe {
            SetEvent(self.0 .0 as HANDLE);
        }
    }
    pub fn wait(&self, timeout: Duration) {
        let handle = self.0 .0 as HANDLE;
        unsafe {
            MsgWaitForMultipleObjectsEx(
                1,
                &handle,
                timeout.as_millis().min(u32::MAX as u128) as u32,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            );
            let mut message = MSG::default();
            // Bound each batch so an SDK message burst cannot starve shutdown
            // or app frames. The next wait returns immediately if more remain.
            for _ in 0..256 {
                if PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) == 0 {
                    break;
                }
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use windows_sys::Win32::{Foundation::*, System::LibraryLoader::GetModuleHandleW};

    static DELIVERED: AtomicUsize = AtomicUsize::new(0);
    unsafe extern "system" fn procedure(
        window: HWND,
        message: u32,
        wp: WPARAM,
        lp: LPARAM,
    ) -> LRESULT {
        if message == WM_APP {
            DELIVERED.fetch_add(1, Ordering::SeqCst);
            return 0;
        }
        unsafe { DefWindowProcW(window, message, wp, lp) }
    }
    #[test]
    fn sdk_worker_dispatches_window_messages_and_app_wakes() {
        std::thread::spawn(|| unsafe {
            let wake = Wake::new().unwrap();
            let class: Vec<u16> = "OpenCADStudio.SpaceMouse.Worker.Test\0"
                .encode_utf16()
                .collect();
            let instance = GetModuleHandleW(std::ptr::null());
            let definition = WNDCLASSW {
                lpfnWndProc: Some(procedure),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                ..Default::default()
            };
            assert_ne!(RegisterClassW(&definition), 0);
            let window = CreateWindowExW(
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
            assert!(!window.is_null());
            assert_ne!(PostMessageW(window, WM_APP, 0, 0), 0);
            wake.wait(Duration::from_secs(1));
            assert_eq!(DELIVERED.load(Ordering::SeqCst), 1);
            let other = wake.clone();
            std::thread::spawn(move || other.signal()).join().unwrap();
            let start = std::time::Instant::now();
            wake.wait(Duration::from_secs(5));
            assert!(start.elapsed() < Duration::from_secs(1));
            DestroyWindow(window);
            UnregisterClassW(class.as_ptr(), instance);
        })
        .join()
        .unwrap();
    }
}
