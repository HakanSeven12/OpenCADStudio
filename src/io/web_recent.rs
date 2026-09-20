//! Browser-backed copies of recently opened drawings.
//!
//! A web file picker exposes bytes and a display name, not a reusable native
//! path. Keep the last-opened copy in the origin-private file system (OPFS) so
//! the Start page can reopen it without asking the user to pick it again.

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

struct OpenStoreState {
    latest_id: u64,
    latest_bytes: std::sync::Arc<[u8]>,
    active_writers: usize,
}

thread_local! {
    static OPEN_STORES: std::cell::RefCell<
        std::collections::HashMap<String, OpenStoreState>,
    > = std::cell::RefCell::new(std::collections::HashMap::new());
}

const RECENT_DIRECTORY: &str = "opencadstudio-recent";
const THUMBNAIL_MAGIC: &[u8; 4] = b"OCST";
const THUMBNAIL_MAX_DIM: u32 = 256;

pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub async fn store(name: &str, bytes: &[u8]) -> Result<(), String> {
    let directory = recent_directory(true).await?;
    write_entry(&directory, &cache_key(name), bytes).await?;

    if let Some(image) = dwg_thumbnailer::extract_bytes(bytes, THUMBNAIL_MAX_DIM) {
        let thumbnail = encode_thumbnail(image);
        // The drawing copy is the durable part of a recent entry. Thumbnail
        // caching must not make an otherwise successful open/save fail under
        // browser quota pressure.
        let _ = write_entry(&directory, &thumbnail_key(name), &thumbnail).await;
    } else {
        // An overwrite may replace a drawing that had a preview with one that
        // does not. Do not leave the old image attached to the new file.
        let _ = JsFuture::from(directory.remove_entry(&thumbnail_key(name))).await;
    }
    Ok(())
}

pub async fn store_open(
    name: &str,
    bytes: std::sync::Arc<[u8]>,
    open_id: u64,
) -> Result<(), String> {
    let key = name.to_string();
    let (mut pending_id, mut pending_bytes) = OPEN_STORES.with(|stores| {
        let mut stores = stores.borrow_mut();
        let state = stores.entry(key.clone()).or_insert_with(|| OpenStoreState {
            latest_id: open_id,
            latest_bytes: std::sync::Arc::clone(&bytes),
            active_writers: 0,
        });
        state.active_writers = state.active_writers.saturating_add(1);
        if open_id > state.latest_id {
            state.latest_id = open_id;
            state.latest_bytes = bytes;
        }
        (state.latest_id, std::sync::Arc::clone(&state.latest_bytes))
    });
    loop {
        if let Err(error) = store(name, &pending_bytes).await {
            OPEN_STORES.with(|stores| {
                let mut stores = stores.borrow_mut();
                let remove = if let Some(state) = stores.get_mut(&key) {
                    state.active_writers = state.active_writers.saturating_sub(1);
                    state.active_writers == 0
                } else {
                    false
                };
                if remove {
                    stores.remove(&key);
                }
            });
            return Err(error);
        }

        let latest = OPEN_STORES.with(|stores| {
            let mut stores = stores.borrow_mut();
            let Some(state) = stores.get_mut(&key) else {
                return None;
            };
            if state.latest_id != pending_id {
                return Some((state.latest_id, std::sync::Arc::clone(&state.latest_bytes)));
            }
            state.active_writers = state.active_writers.saturating_sub(1);
            if state.active_writers == 0 {
                stores.remove(&key);
            }
            None
        });
        match latest {
            Some((latest_id, latest_bytes)) => {
                pending_id = latest_id;
                pending_bytes = latest_bytes;
            }
            None => return Ok(()),
        }
    }
}

async fn write_entry(
    directory: &web_sys::FileSystemDirectoryHandle,
    key: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let options = web_sys::FileSystemGetFileOptions::new();
    options.set_create(true);
    let handle = JsFuture::from(directory.get_file_handle_with_options(key, &options))
        .await
        .map_err(js_error)?
        .dyn_into::<web_sys::FileSystemFileHandle>()
        .map_err(js_error)?;
    let writable = JsFuture::from(handle.create_writable())
        .await
        .map_err(js_error)?
        .dyn_into::<web_sys::FileSystemWritableFileStream>()
        .map_err(js_error)?;
    let write = writable.write_with_u8_array(bytes).map_err(js_error)?;
    JsFuture::from(write).await.map_err(js_error)?;
    JsFuture::from(writable.close()).await.map_err(js_error)?;
    Ok(())
}

pub async fn read(name: &str) -> Result<Vec<u8>, String> {
    let directory = recent_directory(false).await?;
    read_entry(&directory, &cache_key(name)).await
}

async fn read_entry(
    directory: &web_sys::FileSystemDirectoryHandle,
    key: &str,
) -> Result<Vec<u8>, String> {
    let file = get_file(directory, key).await?;
    read_blob(file.as_ref()).await
}

async fn get_file(
    directory: &web_sys::FileSystemDirectoryHandle,
    key: &str,
) -> Result<web_sys::File, String> {
    let handle = JsFuture::from(directory.get_file_handle(key))
        .await
        .map_err(js_error)?
        .dyn_into::<web_sys::FileSystemFileHandle>()
        .map_err(js_error)?;
    JsFuture::from(handle.get_file())
        .await
        .map_err(js_error)?
        .dyn_into::<web_sys::File>()
        .map_err(js_error)
}

async fn read_blob(blob: &web_sys::Blob) -> Result<Vec<u8>, String> {
    let buffer = JsFuture::from(blob.array_buffer())
        .await
        .map_err(js_error)?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

pub async fn read_thumbnail(name: &str) -> Result<Option<Thumbnail>, String> {
    let directory = recent_directory(false).await?;
    match read_entry(&directory, &thumbnail_key(name)).await {
        Ok(bytes) => Ok(decode_thumbnail(bytes)),
        Err(_) => Ok(None),
    }
}

pub async fn remove(name: &str) -> Result<(), String> {
    let directory = recent_directory(false).await?;
    let drawing = JsFuture::from(directory.remove_entry(&cache_key(name)))
        .await
        .map_err(js_error);
    let _ = JsFuture::from(directory.remove_entry(&thumbnail_key(name))).await;
    drawing.map(|_| ())
}

async fn recent_directory(create: bool) -> Result<web_sys::FileSystemDirectoryHandle, String> {
    let window = web_sys::window().ok_or_else(|| "browser window unavailable".to_string())?;
    let root = JsFuture::from(window.navigator().storage().get_directory())
        .await
        .map_err(js_error)?
        .dyn_into::<web_sys::FileSystemDirectoryHandle>()
        .map_err(js_error)?;
    let options = web_sys::FileSystemGetDirectoryOptions::new();
    options.set_create(create);
    JsFuture::from(root.get_directory_handle_with_options(RECENT_DIRECTORY, &options))
        .await
        .map_err(js_error)?
        .dyn_into::<web_sys::FileSystemDirectoryHandle>()
        .map_err(js_error)
}

/// Stable, short OPFS entry name. The original display name remains in
/// `AppConfig::recent`; only the browser-private cache uses this key.
fn cache_key(name: &str) -> String {
    format!("{}.cad", name_hash(name))
}

fn thumbnail_key(name: &str) -> String {
    format!("{}.thumb", name_hash(name))
}

fn name_hash(name: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn encode_thumbnail(image: dwg_thumbnailer::RgbaImage) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(12 + image.as_raw().len());
    bytes.extend_from_slice(THUMBNAIL_MAGIC);
    bytes.extend_from_slice(&image.width().to_le_bytes());
    bytes.extend_from_slice(&image.height().to_le_bytes());
    bytes.extend_from_slice(image.as_raw());
    bytes
}

fn decode_thumbnail(bytes: Vec<u8>) -> Option<Thumbnail> {
    if bytes.get(..4)? != THUMBNAIL_MAGIC {
        return None;
    }
    let width = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?);
    let height = u32::from_le_bytes(bytes.get(8..12)?.try_into().ok()?);
    let expected = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?
        .checked_mul(4)?;
    if width == 0
        || height == 0
        || width > THUMBNAIL_MAX_DIM
        || height > THUMBNAIL_MAX_DIM
        || bytes.len() != 12 + expected
    {
        return None;
    }
    Some(Thumbnail {
        width,
        height,
        rgba: bytes[12..].to_vec(),
    })
}

fn js_error(value: wasm_bindgen::JsValue) -> String {
    value
        .as_string()
        .unwrap_or_else(|| format!("browser storage error: {value:?}"))
}
