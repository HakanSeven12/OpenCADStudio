//! Windows Explorer thumbnail handler for DWG files.
//!
//! Implements `IThumbnailProvider` (+ `IInitializeWithFile`) as a COM in-proc
//! server. Explorer instantiates it for `.dwg` files, hands it the path, then
//! calls `GetThumbnail`, which extracts the DWG's embedded preview via the
//! shared [`dwg_thumbnailer`] core and returns it as an HBITMAP.
//!
//! ## Build & register (on Windows, elevated)
//! ```text
//! cargo build -p dwg-thumbnailer-win --release
//! regsvr32 dwg_thumbnailer_win.dll        :: register
//! regsvr32 /u dwg_thumbnailer_win.dll     :: unregister
//! ```
//! Then restart Explorer (or run `ie4uinit.exe -show`) to refresh thumbnails.
//!
//! NOTE: this module is `cfg(windows)`-only and was authored on a Linux host,
//! so it has NOT been compiled or tested. Build and test it on Windows; minor
//! fixups may be needed for your exact `windows` crate version.

#![cfg(windows)]

use std::cell::RefCell;
use std::ffi::c_void;

use windows::core::{implement, IUnknown, Interface, GUID, HRESULT, PCWSTR};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_INVALIDARG, HMODULE, S_OK,
    WIN32_ERROR,
};
use windows::Win32::Graphics::Gdi::{
    CreateDIBSection, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC,
};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_CLASSES_ROOT,
    KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};
use windows::Win32::UI::Shell::PropertiesSystem::{IInitializeWithFile, IInitializeWithFile_Impl};
use windows::Win32::UI::Shell::{
    IThumbnailProvider, IThumbnailProvider_Impl, SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST,
    WTSAT_ARGB, WTS_ALPHATYPE,
};

/// CLSID of this thumbnail provider (stable; used in the registry keys).
/// {8F2A9C41-3B6E-4E2D-9C7A-1E0B5D6F42AA}
const CLSID_DWG_THUMB: GUID = GUID::from_u128(0x8F2A9C41_3B6E_4E2D_9C7A_1E0B5D6F42AA);
/// The interface id Explorer looks up under `.dwg\ShellEx`.
const IID_ITHUMBNAILPROVIDER: &str = "{e357fccd-a995-4576-b01f-234630154e96}";

/// Our own module handle, captured in `DllMain` — needed to write the DLL path
/// into `InprocServer32` during registration.
static mut SELF_HMODULE: HMODULE = HMODULE(std::ptr::null_mut());

#[no_mangle]
extern "system" fn DllMain(hinst: HMODULE, reason: u32, _reserved: *mut c_void) -> bool {
    if reason == DLL_PROCESS_ATTACH {
        unsafe { SELF_HMODULE = hinst };
    } else if reason == DLL_PROCESS_DETACH {
    }
    true
}

// ── The provider COM object ──────────────────────────────────────────────────

#[implement(IThumbnailProvider, IInitializeWithFile)]
#[derive(Default)]
struct DwgThumbProvider {
    path: RefCell<Option<String>>,
}

impl IInitializeWithFile_Impl for DwgThumbProvider_Impl {
    fn Initialize(&self, pszfilepath: &PCWSTR, _grfmode: u32) -> windows::core::Result<()> {
        let path = unsafe { pszfilepath.to_string() }
            .map_err(|_| windows::core::Error::from(E_INVALIDARG))?;
        *self.path.borrow_mut() = Some(path);
        Ok(())
    }
}

impl IThumbnailProvider_Impl for DwgThumbProvider_Impl {
    fn GetThumbnail(
        &self,
        cx: u32,
        phbmp: *mut HBITMAP,
        pdwalpha: *mut WTS_ALPHATYPE,
    ) -> windows::core::Result<()> {
        let path = self
            .path
            .borrow()
            .clone()
            .ok_or(windows::core::Error::from(E_FAIL))?;
        let mut img = dwg_thumbnailer::extract(std::path::Path::new(&path), cx.max(1))
            .ok_or(windows::core::Error::from(E_FAIL))?;
        dwg_thumbnailer::badge_dwg(&mut img); // full-width "DWG" band
        let hbmp = unsafe { rgba_to_hbitmap(&img)? };
        unsafe {
            *phbmp = hbmp;
            *pdwalpha = WTSAT_ARGB;
        }
        Ok(())
    }
}

/// Build a 32-bit top-down BGRA `HBITMAP` from an RGBA image.
unsafe fn rgba_to_hbitmap(img: &dwg_thumbnailer::RgbaImage) -> windows::core::Result<HBITMAP> {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // negative → top-down rows
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = std::ptr::null_mut();
    let hbmp = CreateDIBSection(HDC::default(), &bi, DIB_RGB_COLORS, &mut bits, None, 0)?;
    if bits.is_null() {
        return Err(E_FAIL.into());
    }
    let dst = std::slice::from_raw_parts_mut(bits as *mut u8, (w * h * 4) as usize);
    for (i, px) in img.pixels().enumerate() {
        let [r, g, b, a] = px.0;
        dst[i * 4] = b;
        dst[i * 4 + 1] = g;
        dst[i * 4 + 2] = r;
        dst[i * 4 + 3] = a;
    }
    Ok(hbmp)
}

// ── Class factory ────────────────────────────────────────────────────────────

#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        punkouter: Option<&IUnknown>,
        riid: *const GUID,
        ppvobject: *mut *mut c_void,
    ) -> windows::core::Result<()> {
        if punkouter.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let provider: IUnknown = DwgThumbProvider::default().into();
        unsafe { provider.query(&*riid, ppvobject).ok() }
    }

    fn LockServer(&self, _flock: windows::Win32::Foundation::BOOL) -> windows::core::Result<()> {
        Ok(())
    }
}

// ── DLL exports ──────────────────────────────────────────────────────────────

#[no_mangle]
extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    unsafe {
        if *rclsid != CLSID_DWG_THUMB {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
        let factory: IClassFactory = Factory.into();
        factory.query(&*riid, ppv)
    }
}

#[no_mangle]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    // Simplification: report unloadable only when COM has released everything.
    // A conservative always-`S_FALSE` keeps the DLL resident (safe, if leakier).
    windows::Win32::Foundation::S_FALSE
}

#[no_mangle]
extern "system" fn DllRegisterServer() -> HRESULT {
    match register(true) {
        Ok(()) => S_OK,
        Err(e) => e.code(),
    }
}

#[no_mangle]
extern "system" fn DllUnregisterServer() -> HRESULT {
    match register(false) {
        Ok(()) => S_OK,
        Err(e) => e.code(),
    }
}

// ── Registration (HKCR) ──────────────────────────────────────────────────────

fn module_path() -> windows::core::Result<String> {
    let mut buf = [0u16; 1024];
    let len = unsafe { GetModuleFileNameW(SELF_HMODULE, &mut buf) };
    if len == 0 {
        return Err(E_FAIL.into());
    }
    Ok(String::from_utf16_lossy(&buf[..len as usize]))
}

fn register(install: bool) -> windows::core::Result<()> {
    let clsid = format!("{{{:?}}}", CLSID_DWG_THUMB); // "{8F2A9C41-...}"
    let clsid_key = format!("CLSID\\{clsid}");
    let inproc_key = format!("{clsid_key}\\InprocServer32");
    let dwg_shellex = format!(".dwg\\ShellEx\\{IID_ITHUMBNAILPROVIDER}");

    if install {
        let dll = module_path()?;
        set_value(&clsid_key, None, "OpenCADStudio DWG Thumbnail Provider")?;
        set_value(&inproc_key, None, &dll)?;
        set_value(&inproc_key, Some("ThreadingModel"), "Apartment")?;
        set_value(&dwg_shellex, None, &clsid)?;
    } else {
        let _ = delete_tree(&clsid_key);
        let _ = delete_tree(&dwg_shellex);
    }
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
    Ok(())
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn set_value(sub: &str, name: Option<&str>, value: &str) -> windows::core::Result<()> {
    let sub_w = wide(sub);
    let mut hkey = HKEY::default();
    let rc = unsafe {
        RegCreateKeyExW(
            HKEY_CLASSES_ROOT,
            PCWSTR(sub_w.as_ptr()),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut hkey,
            None,
        )
    };
    if rc != WIN32_ERROR(0) {
        return Err(E_FAIL.into());
    }
    let val_w = wide(value);
    let bytes = unsafe { std::slice::from_raw_parts(val_w.as_ptr() as *const u8, val_w.len() * 2) };
    let name_w = name.map(wide);
    let rc = unsafe {
        RegSetValueExW(
            hkey,
            name_w
                .as_ref()
                .map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr())),
            0,
            REG_SZ,
            Some(bytes),
        )
    };
    unsafe {
        let _ = RegCloseKey(hkey);
    }
    if rc != WIN32_ERROR(0) {
        return Err(E_FAIL.into());
    }
    Ok(())
}

fn delete_tree(sub: &str) -> windows::core::Result<()> {
    let sub_w = wide(sub);
    let rc = unsafe { RegDeleteTreeW(HKEY_CLASSES_ROOT, PCWSTR(sub_w.as_ptr())) };
    if rc != WIN32_ERROR(0) {
        return Err(E_FAIL.into());
    }
    Ok(())
}
