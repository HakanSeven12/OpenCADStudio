//! Windows Explorer thumbnail handler for DWG files.
//!
//! Implements `IThumbnailProvider` (+ `IInitializeWithStream`) as a COM in-proc
//! server. Explorer instantiates it for `.dwg` files in its isolated thumbnail
//! process, hands it the file as a stream, then calls `GetThumbnail`, which
//! seeks directly to the DWG's embedded preview via the shared
//! [`dwg_thumbnailer`] core and returns it as an `HBITMAP`.
//!
//! ## Build & register (on Windows)
//! ```text
//! cargo build -p dwg-thumbnailer-win --release
//! regsvr32 dwg_thumbnailer_win.dll        :: register (HKLM if elevated, else HKCU)
//! regsvr32 /u dwg_thumbnailer_win.dll     :: unregister
//! ```
//! Then restart Explorer (or run `ie4uinit.exe -show`) to refresh thumbnails.

/// CLSID of this thumbnail provider (stable; used in the registry keys).
pub const CLSID_DWG_THUMB_STR: &str = "{8F2A9C41-3B6E-4E2D-9C7A-1E0B5D6F42AA}";
/// The interface id Explorer looks up under `.dwg\ShellEx`.
pub const IID_ITHUMBNAILPROVIDER: &str = "{e357fccd-a995-4576-b01f-234630154e96}";

/// Upper height allowed for a badged thumbnail (`MAX_THUMBNAIL_DIM` + band).
const MAX_BADGED_HEIGHT: u32 =
    dwg_thumbnailer::MAX_THUMBNAIL_DIM + dwg_thumbnailer::MAX_THUMBNAIL_DIM / 4 + 32;

/// Validate RGBA image dimensions before allocating a top-down 32-bit DIB
/// section, computing `(width, negative_height, byte_len)` with checked math.
pub fn checked_rgba_bitmap_metrics(w: u32, h: u32) -> Option<(i32, i32, usize)> {
    if w == 0
        || h == 0
        || w > dwg_thumbnailer::MAX_THUMBNAIL_DIM
        || h > MAX_BADGED_HEIGHT
    {
        return None;
    }
    let w_i32 = i32::try_from(w).ok()?;
    let h_i32 = i32::try_from(h).ok()?;
    let neg_h = h_i32.checked_neg()?;
    let pixels = usize::try_from(w)
        .ok()?
        .checked_mul(usize::try_from(h).ok()?)?;
    let byte_len = pixels.checked_mul(4)?;
    Some((w_i32, neg_h, byte_len))
}

/// Explicit `Software\Classes\...` registry paths written under either
/// `HKEY_LOCAL_MACHINE` (elevated) or `HKEY_CURRENT_USER` (per-user), never
/// via merged `HKEY_CLASSES_ROOT`.
pub fn registration_subkeys(clsid: &str) -> (String, String, String) {
    let clsid_key = format!("Software\\Classes\\CLSID\\{clsid}");
    let inproc_key = format!("{clsid_key}\\InprocServer32");
    let dwg_shellex = format!("Software\\Classes\\.dwg\\ShellEx\\{IID_ITHUMBNAILPROVIDER}");
    (clsid_key, inproc_key, dwg_shellex)
}

#[cfg(windows)]
mod win {
    use super::{checked_rgba_bitmap_metrics, registration_subkeys, CLSID_DWG_THUMB_STR};
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::io::SeekFrom;
    use std::sync::atomic::{AtomicPtr, Ordering};

    use windows::core::{implement, IUnknown, Interface, GUID, HRESULT, PCWSTR};
    use windows::Win32::Foundation::{
        CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, ERROR_ACCESS_DENIED,
        ERROR_FILE_NOT_FOUND, E_ACCESSDENIED, E_FAIL, E_INVALIDARG, E_POINTER, HMODULE, S_FALSE,
        S_OK, WIN32_ERROR,
    };
    use windows::Win32::Graphics::Gdi::{
        CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        HBITMAP, HGDIOBJ,
    };
    use windows::Win32::System::Com::{
        IClassFactory, IClassFactory_Impl, IStream, STREAM_SEEK_CUR, STREAM_SEEK_END,
        STREAM_SEEK_SET,
    };
    use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        HKEY_LOCAL_MACHINE, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
    };
    use windows::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};
    use windows::Win32::UI::Shell::PropertiesSystem::{
        IInitializeWithStream, IInitializeWithStream_Impl,
    };
    use windows::Win32::UI::Shell::{
        IThumbnailProvider, IThumbnailProvider_Impl, SHChangeNotify, SHCNE_ASSOCCHANGED,
        SHCNF_IDLIST, WTSAT_ARGB, WTS_ALPHATYPE,
    };

    /// CLSID of this thumbnail provider (stable; used in the registry keys).
    /// {8F2A9C41-3B6E-4E2D-9C7A-1E0B5D6F42AA}
    const CLSID_DWG_THUMB: GUID = GUID::from_u128(0x8F2A9C41_3B6E_4E2D_9C7A_1E0B5D6F42AA);

    /// Our own module handle, captured in `DllMain` — needed to write the DLL path
    /// into `InprocServer32` during registration.
    static SELF_HMODULE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

    #[no_mangle]
    pub extern "system" fn DllMain(hinst: HMODULE, reason: u32, _reserved: *mut c_void) -> bool {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if reason == DLL_PROCESS_ATTACH {
                SELF_HMODULE.store(hinst.0, Ordering::Release);
            } else if reason == DLL_PROCESS_DETACH {
            }
            true
        }))
        .unwrap_or(false)
    }

    // ── Stream adapter + provider COM object ──────────────────────────────────

    struct ComStreamReader<'a>(&'a IStream);

    impl std::io::Read for ComStreamReader<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let want = u32::try_from(buf.len()).unwrap_or(u32::MAX);
            let mut read = 0u32;
            // SAFETY: `self.0` is a valid COM `IStream` and `buf` provides `want` writable bytes.
            unsafe {
                self.0
                    .Read(buf.as_mut_ptr().cast(), want, Some(&mut read))
                    .ok()
                    .map_err(std::io::Error::other)?;
            }
            Ok(read as usize)
        }
    }

    impl std::io::Seek for ComStreamReader<'_> {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            let (offset, origin) = match pos {
                SeekFrom::Start(n) => {
                    let delta = i64::try_from(n).map_err(|_| {
                        std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek overflow")
                    })?;
                    (delta, STREAM_SEEK_SET)
                }
                SeekFrom::Current(n) => (n, STREAM_SEEK_CUR),
                SeekFrom::End(n) => (n, STREAM_SEEK_END),
            };
            let mut new_pos = 0u64;
            // SAFETY: `self.0` is a valid COM `IStream`.
            unsafe {
                self.0
                    .Seek(offset, origin, Some(&mut new_pos))
                    .map_err(std::io::Error::other)?;
            }
            Ok(new_pos)
        }
    }

    // Explorer loads thumbnail providers in an isolated process, where only a
    // stream initializer is offered; a path initializer is never called there.
    #[implement(IThumbnailProvider, IInitializeWithStream)]
    #[derive(Default)]
    struct DwgThumbProvider {
        preview: RefCell<Option<dwg_thumbnailer::PreviewData>>,
    }

    impl IInitializeWithStream_Impl for DwgThumbProvider_Impl {
        fn Initialize(
            &self,
            pstream: windows::core::Ref<'_, IStream>,
            _grfmode: u32,
        ) -> windows::core::Result<()> {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let stream = pstream
                    .as_ref()
                    .ok_or_else(|| windows::core::Error::from(E_INVALIDARG))?;
                let mut reader = ComStreamReader(stream);
                let preview = dwg_thumbnailer::PreviewData::read_from(&mut reader);
                *self.preview.borrow_mut() = preview;
                Ok(())
            }))
            .unwrap_or_else(|_| Err(E_FAIL.into()))
        }
    }

    impl IThumbnailProvider_Impl for DwgThumbProvider_Impl {
        fn GetThumbnail(
            &self,
            cx: u32,
            phbmp: *mut HBITMAP,
            pdwalpha: *mut WTS_ALPHATYPE,
        ) -> windows::core::Result<()> {
            if phbmp.is_null() || pdwalpha.is_null() {
                return Err(E_POINTER.into());
            }
            // SAFETY: `phbmp` and `pdwalpha` were checked non-null per COM contract.
            unsafe {
                *phbmp = HBITMAP::default();
                *pdwalpha = WTSAT_ARGB;
            }
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let preview = self.preview.borrow();
                let preview = preview
                    .as_ref()
                    .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
                let max_dim = cx.clamp(1, dwg_thumbnailer::MAX_THUMBNAIL_DIM);
                let mut img = preview
                    .decode_scaled(max_dim)
                    .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
                // Explorer draws the app icon on the bottom-right corner: keep "DWG" clear of it.
                dwg_thumbnailer::badge_dwg_clear_of(
                    &mut img,
                    dwg_thumbnailer::WINDOWS_TYPE_OVERLAY,
                );
                // SAFETY: `img` holds valid RGBA8 pixel storage.
                let hbmp = unsafe { rgba_to_hbitmap(&img)? };
                // SAFETY: `phbmp` and `pdwalpha` are non-null writable COM out-pointers.
                unsafe {
                    *phbmp = hbmp;
                    *pdwalpha = WTSAT_ARGB;
                }
                Ok(())
            }))
            .unwrap_or_else(|_| Err(E_FAIL.into()))
        }
    }

    /// Build a 32-bit top-down BGRA `HBITMAP` from an RGBA image.
    ///
    /// # Safety
    /// Calls Win32 GDI `CreateDIBSection` and writes `w * h * 4` bytes to the
    /// returned DIB pixel memory.
    unsafe fn rgba_to_hbitmap(img: &dwg_thumbnailer::RgbaImage) -> windows::core::Result<HBITMAP> {
        let (w, neg_h, byte_len) = checked_rgba_bitmap_metrics(img.width(), img.height())
            .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        let bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: neg_h, // negative → top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let hbmp = CreateDIBSection(None, &bi, DIB_RGB_COLORS, &mut bits, None, 0)?;
        if bits.is_null() {
            let _ = DeleteObject(HGDIOBJ(hbmp.0));
            return Err(E_FAIL.into());
        }
        // SAFETY: `CreateDIBSection` allocated `byte_len` bytes for a 32-bpp `w x h` bitmap.
        let dst = std::slice::from_raw_parts_mut(bits as *mut u8, byte_len);
        for (dst_px, px) in dst.chunks_exact_mut(4).zip(img.pixels()) {
            let [r, g, b, a] = px.0;
            dst_px[0] = b;
            dst_px[1] = g;
            dst_px[2] = r;
            dst_px[3] = a;
        }
        Ok(hbmp)
    }

    // ── Class factory ────────────────────────────────────────────────────────

    #[implement(IClassFactory)]
    struct Factory;

    impl IClassFactory_Impl for Factory_Impl {
        fn CreateInstance(
            &self,
            punkouter: windows::core::Ref<'_, IUnknown>,
            riid: *const GUID,
            ppvobject: *mut *mut c_void,
        ) -> windows::core::Result<()> {
            if ppvobject.is_null() || riid.is_null() {
                return Err(E_POINTER.into());
            }
            // SAFETY: `ppvobject` is non-null per COM contract.
            unsafe { *ppvobject = std::ptr::null_mut() };
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if !punkouter.is_null() {
                    return Err(CLASS_E_NOAGGREGATION.into());
                }
                let provider: IUnknown = DwgThumbProvider::default().into();
                // SAFETY: `riid` and `ppvobject` were checked non-null.
                unsafe { provider.query(&*riid, ppvobject).ok() }
            }))
            .unwrap_or_else(|_| Err(E_FAIL.into()))
        }

        fn LockServer(&self, _flock: windows::core::BOOL) -> windows::core::Result<()> {
            Ok(())
        }
    }

    // ── DLL exports ──────────────────────────────────────────────────────────

    #[no_mangle]
    pub extern "system" fn DllGetClassObject(
        rclsid: *const GUID,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> HRESULT {
        if !ppv.is_null() {
            // SAFETY: `ppv` was checked non-null.
            unsafe { *ppv = std::ptr::null_mut() };
        }
        if rclsid.is_null() || riid.is_null() || ppv.is_null() {
            return E_POINTER;
        }
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // SAFETY: `rclsid`, `riid`, and `ppv` are non-null COM pointers.
            unsafe {
                if *rclsid != CLSID_DWG_THUMB {
                    return CLASS_E_CLASSNOTAVAILABLE;
                }
                let factory: IClassFactory = Factory.into();
                factory.query(&*riid, ppv)
            }
        }))
        .unwrap_or(E_FAIL)
    }

    #[no_mangle]
    pub extern "system" fn DllCanUnloadNow() -> HRESULT {
        S_FALSE
    }

    #[no_mangle]
    pub extern "system" fn DllRegisterServer() -> HRESULT {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match register(true) {
            Ok(()) => S_OK,
            Err(e) => e.code(),
        }))
        .unwrap_or(E_FAIL)
    }

    #[no_mangle]
    pub extern "system" fn DllUnregisterServer() -> HRESULT {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match register(false) {
            Ok(()) => S_OK,
            Err(e) => e.code(),
        }))
        .unwrap_or(E_FAIL)
    }

    // ── Registration (explicit HKLM / HKCU Software\Classes) ─────────────────

    fn module_path() -> windows::core::Result<String> {
        let handle = SELF_HMODULE.load(Ordering::Acquire);
        if handle.is_null() {
            return Err(E_FAIL.into());
        }
        let mut buf = [0u16; 1024];
        // SAFETY: `handle` is our loaded HMODULE and `buf` is stack-allocated.
        let len = unsafe { GetModuleFileNameW(Some(HMODULE(handle)), &mut buf) };
        if len == 0 || (len as usize) >= buf.len() {
            return Err(E_FAIL.into());
        }
        Ok(String::from_utf16_lossy(&buf[..len as usize]))
    }

    fn register(install: bool) -> windows::core::Result<()> {
        if install {
            let dll = module_path()?;
            match register_in_root(HKEY_LOCAL_MACHINE, true, Some(&dll)) {
                Ok(()) => {}
                Err(e) if e.code() == E_ACCESSDENIED => {
                    register_in_root(HKEY_CURRENT_USER, true, Some(&dll))?;
                }
                Err(e) => return Err(e),
            }
        } else {
            let hklm_res = register_in_root(HKEY_LOCAL_MACHINE, false, None);
            let hkcu_res = register_in_root(HKEY_CURRENT_USER, false, None);
            match (hklm_res, hkcu_res) {
                (Ok(()), _) | (_, Ok(())) => {}
                (Err(e), _) => return Err(e),
            }
        }
        // SAFETY: standard Win32 shell notification with null item lists.
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
        Ok(())
    }

    fn register_in_root(
        root: HKEY,
        install: bool,
        dll: Option<&str>,
    ) -> windows::core::Result<()> {
        let clsid = CLSID_DWG_THUMB_STR;
        let (clsid_key, inproc_key, dwg_shellex) = registration_subkeys(clsid);
        if install {
            let dll = dll.ok_or_else(|| windows::core::Error::from(E_FAIL))?;
            set_value(
                root,
                &clsid_key,
                None,
                "OpenCADStudio DWG Thumbnail Provider",
            )?;
            set_value(root, &inproc_key, None, dll)?;
            set_value(root, &inproc_key, Some("ThreadingModel"), "Apartment")?;
            set_value(root, &dwg_shellex, None, clsid)?;
        } else {
            let mut access_denied = false;
            for key in [&clsid_key, &dwg_shellex] {
                if let Err(e) = delete_tree(root, key) {
                    if e.code() == E_ACCESSDENIED {
                        access_denied = true;
                    } else {
                        return Err(e);
                    }
                }
            }
            if access_denied {
                return Err(E_ACCESSDENIED.into());
            }
        }
        Ok(())
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn set_value(
        root: HKEY,
        sub: &str,
        name: Option<&str>,
        value: &str,
    ) -> windows::core::Result<()> {
        let sub_w = wide(sub);
        let mut hkey = HKEY::default();
        // SAFETY: `sub_w` is NUL-terminated UTF-16 and `hkey` receives the opened handle.
        let rc = unsafe {
            RegCreateKeyExW(
                root,
                PCWSTR(sub_w.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut hkey,
                None,
            )
        };
        if rc == ERROR_ACCESS_DENIED {
            return Err(E_ACCESSDENIED.into());
        }
        if rc != WIN32_ERROR(0) {
            return Err(E_FAIL.into());
        }
        let val_w = wide(value);
        // SAFETY: `val_w` outlives `bytes` and `val_w.len() * 2` covers its UTF-16 bytes.
        let bytes =
            unsafe { std::slice::from_raw_parts(val_w.as_ptr() as *const u8, val_w.len() * 2) };
        let name_w = name.map(wide);
        // SAFETY: `hkey`, `name_w`, and `bytes` remain valid across the call.
        let rc = unsafe {
            RegSetValueExW(
                hkey,
                name_w
                    .as_ref()
                    .map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr())),
                None,
                REG_SZ,
                Some(bytes),
            )
        };
        // SAFETY: `hkey` was opened by `RegCreateKeyExW` above.
        unsafe {
            let _ = RegCloseKey(hkey);
        }
        if rc == ERROR_ACCESS_DENIED {
            return Err(E_ACCESSDENIED.into());
        }
        if rc != WIN32_ERROR(0) {
            return Err(E_FAIL.into());
        }
        Ok(())
    }

    fn delete_tree(root: HKEY, sub: &str) -> windows::core::Result<()> {
        let sub_w = wide(sub);
        // SAFETY: `sub_w` is NUL-terminated UTF-16.
        let rc = unsafe { RegDeleteTreeW(root, PCWSTR(sub_w.as_ptr())) };
        if rc == WIN32_ERROR(0) || rc == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        if rc == ERROR_ACCESS_DENIED {
            return Err(E_ACCESSDENIED.into());
        }
        Err(E_FAIL.into())
    }

    #[cfg(test)]
    mod win_tests {
        use super::*;
        use windows::Win32::Foundation::HGLOBAL;
        use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;

        #[test]
        fn clsid_constant_matches_guid_formatting() {
            let formatted = format!("{{{:?}}}", CLSID_DWG_THUMB);
            assert!(formatted.eq_ignore_ascii_case(CLSID_DWG_THUMB_STR));
        }

        #[test]
        fn com_exports_reject_null_pointers_and_render_synthetic_dwg_stream() {
            let mut ppv: *mut c_void = std::ptr::null_mut();
            let iid_cf = IClassFactory::IID;
            assert_eq!(
                DllGetClassObject(std::ptr::null(), &iid_cf, &mut ppv),
                E_POINTER
            );
            assert_eq!(
                DllGetClassObject(&CLSID_DWG_THUMB, std::ptr::null(), &mut ppv),
                E_POINTER
            );
            assert_eq!(
                DllGetClassObject(&CLSID_DWG_THUMB, &iid_cf, std::ptr::null_mut()),
                E_POINTER
            );
            let wrong_guid = GUID::zeroed();
            assert_eq!(
                DllGetClassObject(&wrong_guid, &iid_cf, &mut ppv),
                CLASS_E_CLASSNOTAVAILABLE
            );

            assert_eq!(DllGetClassObject(&CLSID_DWG_THUMB, &iid_cf, &mut ppv), S_OK);
            assert!(!ppv.is_null());
            // SAFETY: `DllGetClassObject` returned a valid `IClassFactory` pointer.
            let factory: IClassFactory = unsafe { IClassFactory::from_raw(ppv) };

            // SAFETY: invoke raw `IClassFactory::CreateInstance` with a null `ppvObject`
            // to verify `E_POINTER` without relying on the safe wrapper's typed out-param.
            let raw_hr = unsafe {
                (Interface::vtable(&factory).CreateInstance)(
                    Interface::as_raw(&factory),
                    std::ptr::null_mut(),
                    &IThumbnailProvider::IID,
                    std::ptr::null_mut(),
                )
            };
            assert_eq!(raw_hr, E_POINTER);

            // SAFETY: `CreateInstance` allocates an unaggregated `IThumbnailProvider`.
            let thumb: IThumbnailProvider = unsafe {
                factory
                    .CreateInstance(None)
                    .expect("CreateInstance succeeds")
            };
            let init: IInitializeWithStream = thumb.cast().expect("IInitializeWithStream QI");

            let png = include_bytes!("../../dwg-thumbnailer/assets/dwg-label.png");
            let sentinel: [u8; 16] = [
                0x1F, 0x25, 0x6D, 0x07, 0xD4, 0x36, 0x28, 0x28, 0x9D, 0x57, 0xCA, 0x3F, 0x9D,
                0x44, 0x10, 0x2B,
            ];
            let base = 128usize;
            let overall = 1 + 9 + png.len();
            let mut dwg = vec![0u8; base + 36 + overall];
            dwg[..6].copy_from_slice(b"AC1032");
            dwg[0x0D..0x11].copy_from_slice(&(base as i32).to_le_bytes());
            dwg[base..base + 16].copy_from_slice(&sentinel);
            dwg[base + 16..base + 20].copy_from_slice(&(overall as u32).to_le_bytes());
            dwg[base + 20] = 1;
            dwg[base + 21] = 6;
            let start = base + 21 + 9;
            dwg[base + 22..base + 26].copy_from_slice(&(start as u32).to_le_bytes());
            dwg[base + 26..base + 30].copy_from_slice(&(png.len() as u32).to_le_bytes());
            dwg[start..start + png.len()].copy_from_slice(png);

            // SAFETY: creates an empty in-memory `IStream`, writes `dwg`, and rewinds to 0.
            let stream = unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true) }.unwrap();
            unsafe {
                let mut written = 0u32;
                stream
                    .Write(dwg.as_ptr().cast(), dwg.len() as u32, Some(&mut written))
                    .ok()
                    .unwrap();
                stream.Seek(0, STREAM_SEEK_SET, None).unwrap();
                init.Initialize(&stream, 0).expect("Initialize succeeds");
            }

            let mut hbmp = HBITMAP::default();
            let mut alpha = WTSAT_ARGB;
            assert!(unsafe { thumb.GetThumbnail(128, std::ptr::null_mut(), &mut alpha) }.is_err());
            assert!(unsafe { thumb.GetThumbnail(128, &mut hbmp, std::ptr::null_mut()) }.is_err());
            unsafe {
                thumb
                    .GetThumbnail(u32::MAX, &mut hbmp, &mut alpha)
                    .expect("GetThumbnail succeeds even with oversized cx");
                assert!(!hbmp.is_invalid());
                let _ = DeleteObject(HGDIOBJ(hbmp.0));
            }
        }
    }
}

#[cfg(windows)]
pub use win::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_rgba_bitmap_metrics_guards_zero_and_overflow_dims() {
        assert!(checked_rgba_bitmap_metrics(0, 10).is_none());
        assert!(checked_rgba_bitmap_metrics(10, 0).is_none());
        assert!(checked_rgba_bitmap_metrics(dwg_thumbnailer::MAX_THUMBNAIL_DIM + 1, 10).is_none());
        assert!(checked_rgba_bitmap_metrics(10, MAX_BADGED_HEIGHT + 1).is_none());
        assert_eq!(checked_rgba_bitmap_metrics(32, 48), Some((32, -48, 32 * 48 * 4)));
    }

    #[test]
    fn registration_subkeys_target_explicit_software_classes() {
        let (clsid_key, inproc_key, dwg_shellex) = registration_subkeys(CLSID_DWG_THUMB_STR);
        assert!(clsid_key.starts_with(r"Software\Classes\CLSID\"));
        assert!(inproc_key.starts_with(&clsid_key));
        assert!(dwg_shellex.starts_with(r"Software\Classes\.dwg\ShellEx\"));
    }
}
