//! Encrypted PDFs (standard security handler): the document (user) and
//! permission (owner) password checks and the passwords supplied this
//! session.
//!
//! A file is attached with document access and imported (its geometry
//! extracted) only with owner access. The reader opens a file with its user
//! password; for revisions 2–4 the owner password recovers the user password
//! from the `/O` entry (it is not a user password itself), for revisions 5–6
//! the reader accepts either.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use hayro::hayro_syntax::{LoadPdfError, Pdf};

const PADDING: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08,
    0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

/// The password a dialog asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// The password that opens the document (user password).
    Document,
    /// The password that removes its restrictions (owner password).
    Permission,
}

/// What opening a PDF for attach or import needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Not encrypted, or already unlocked.
    Open,
    /// Unlocked with a password supplied earlier this session.
    Remembered,
    /// A password is needed; `remembered` when a password supplied earlier
    /// opened the document but did not remove its restrictions.
    Ask { stage: Stage, remembered: bool },
}

/// A password typed in the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// The access asked for: the file opens.
    Unlocked,
    /// The document password: its restrictions still need the owner one.
    NeedPermission,
    Wrong,
}

/// A password that opened a file.
struct Grant {
    /// As supplied: the access checks run on it.
    typed: String,
    /// The user password the reader opens the file with.
    reader: String,
}

#[derive(Default)]
struct Session {
    /// Passwords tried on every PDF, in the order they were supplied.
    remembered: Vec<String>,
    /// The password that opened each file (also when not remembered for
    /// other files): the reader needs it again on every read.
    granted: HashMap<String, Grant>,
    /// A file just unlocked in the dialog: its next check passes without the
    /// "previously supplied" message.
    fresh: Option<String>,
}

fn session() -> &'static Mutex<Session> {
    static SESSION: OnceLock<Mutex<Session>> = OnceLock::new();
    SESSION.get_or_init(|| Mutex::new(Session::default()))
}

fn lock() -> std::sync::MutexGuard<'static, Session> {
    session().lock().unwrap_or_else(|e| e.into_inner())
}

/// The file a derived source (`<file>#layers-off=…`) was made from.
fn file_key(path: &str) -> &str {
    path.split_once("#layers-off=")
        .map_or(path, |(file, _)| file)
}

/// Keep `typed` as the password that opened `path`.
fn grant(path: &str, security: &Security, bytes: &Arc<Vec<u8>>, typed: &str) {
    let reader = security
        .user_password(bytes, typed)
        .unwrap_or_else(|| typed.to_string());
    let typed = typed.to_string();
    lock()
        .granted
        .insert(path.to_string(), Grant { typed, reader });
}

/// Open a PDF source, trying no password, then the one that unlocked it,
/// then every password supplied this session.
pub fn open(path: &str, bytes: Arc<Vec<u8>>) -> Result<Pdf, LoadPdfError> {
    let first = Pdf::new(bytes.clone());
    if !matches!(first, Err(LoadPdfError::Decryption(_))) {
        return first;
    }
    let key = file_key(path);
    let (reader, remembered) = {
        let session = lock();
        let reader = session.granted.get(key).map(|g| g.reader.clone());
        (reader, session.remembered.clone())
    };
    if let Some(Ok(pdf)) = reader.map(|user| Pdf::new_with_password(bytes.clone(), &user)) {
        return Ok(pdf);
    }
    let Some(security) = Security::read(&bytes) else {
        return first;
    };
    for password in remembered {
        if let Some(user) = security.user_password(&bytes, &password) {
            if let Ok(pdf) = Pdf::new_with_password(bytes.clone(), &user) {
                let grant = Grant {
                    typed: password,
                    reader: user,
                };
                lock().granted.insert(key.to_string(), grant);
                return Ok(pdf);
            }
        }
    }
    first
}

/// The lines a file prompt prints when a password supplied earlier
/// unlocked the file.
pub fn remembered_notice(path: &str) -> String {
    crate::tr!("pdf-password", "remembered", path = path)
}

/// The notice importing a page of `path` prints: the file is opened again
/// with the password supplied for it whenever it cannot be read without one.
pub fn reopened_notice(path: &str) -> Option<String> {
    let bytes = super::pdf_raster::source_bytes(path)?;
    let security = Security::read(&bytes)?;
    (!security.is_owner(&bytes, "")).then(|| remembered_notice(path))
}

/// The lines a file prompt prints when the password window is cancelled;
/// the prompt is then asked again.
pub fn refused_notice(path: &str) -> String {
    crate::tr!("pdf-password", "refused", path = path)
}

/// A file prompt's answer for `path` (typed as `input`): the password
/// request, or the notice to print once the file is read. `attach`: document
/// access is enough and cancelling the window prints that the file is
/// encrypted; otherwise owner access is needed.
pub fn prompt_gate(
    path: &str,
    input: &str,
    attach: bool,
) -> Result<Option<String>, crate::command::CmdResult> {
    match gate(path, !attach) {
        Gate::Open => Ok(None),
        Gate::Remembered => Ok(Some(remembered_notice(path))),
        Gate::Ask { stage, remembered } => Err(crate::command::CmdResult::PdfPassword {
            path: path.to_string(),
            input: input.to_string(),
            stage,
            remembered,
            attach,
        }),
    }
}

/// What attaching (`owner` false) or importing (`owner` true) `path` needs
/// before its pages are read.
pub fn gate(path: &str, owner: bool) -> Gate {
    let Some(bytes) = super::pdf_raster::source_bytes(path) else {
        return Gate::Open;
    };
    let Some(security) = Security::read(&bytes) else {
        return Gate::Open;
    };
    let candidates = {
        let mut session = lock();
        if session.fresh.as_deref() == Some(path) {
            session.fresh = None;
            return Gate::Open;
        }
        let granted = session.granted.get(path).map(|g| g.typed.clone());
        let mut list: Vec<String> = granted.into_iter().collect();
        list.extend(session.remembered.iter().cloned());
        list
    };
    let access = |p: &str| security.is_owner(&bytes, p) || (!owner && security.is_user(&bytes, p));
    if access("") {
        return Gate::Open;
    }
    if let Some(password) = candidates.iter().find(|p| access(p)) {
        grant(path, &security, &bytes, password);
        return Gate::Remembered;
    }
    if !owner {
        return Gate::Ask {
            stage: Stage::Document,
            remembered: false,
        };
    }
    if security.is_user(&bytes, "") {
        return Gate::Ask {
            stage: Stage::Permission,
            remembered: false,
        };
    }
    let remembered = candidates.iter().any(|p| security.is_user(&bytes, p));
    Gate::Ask {
        stage: if remembered {
            Stage::Permission
        } else {
            Stage::Document
        },
        remembered,
    }
}

/// Check a password typed for `stage`; `owner` as in [`gate`]. A document
/// password is kept for the file (and for every file when `remember`).
pub fn check(path: &str, stage: Stage, password: &str, remember: bool, owner: bool) -> Check {
    let Some(bytes) = super::pdf_raster::source_bytes(path) else {
        return Check::Wrong;
    };
    let Some(security) = Security::read(&bytes) else {
        return Check::Unlocked;
    };
    let keep = || {
        grant(path, &security, &bytes, password);
        let mut session = lock();
        if remember && !session.remembered.iter().any(|p| p == password) {
            session.remembered.push(password.to_string());
        }
    };
    let user = security.is_user(&bytes, password);
    if security.is_owner(&bytes, password) || (!owner && user) {
        keep();
        lock().fresh = Some(path.to_string());
        // Pages read while the file was locked are read again.
        super::pdf_raster::register_source(path, bytes);
        return Check::Unlocked;
    }
    if stage == Stage::Document && user {
        keep();
        return Check::NeedPermission;
    }
    Check::Wrong
}

/// The encryption dictionary's entries the password checks use.
struct Security {
    revision: u8,
    /// File key length in bytes (revisions 2–4).
    key_len: usize,
    owner: Vec<u8>,
    user: Vec<u8>,
}

impl Security {
    /// `None` for a file without an encryption dictionary.
    // ponytail: byte search over the file for the trailer's /Encrypt and the
    // dictionary object; a dictionary inside a compressed object stream (not
    // allowed for /Encrypt) would be missed.
    fn read(data: &[u8]) -> Option<Self> {
        let at = rfind_key(data, b"/Encrypt")?;
        let rest = skip_ws(data, at + b"/Encrypt".len());
        let dict = if data[rest..].starts_with(b"<<") {
            &data[rest..]
        } else {
            let (num, after) = read_int(data, rest)?;
            let (gen, after) = read_int(data, skip_ws(data, after))?;
            if data.get(skip_ws(data, after)) != Some(&b'R') {
                return None;
            }
            let marker = format!("{num} {gen} obj");
            let mut found = None;
            let mut from = 0;
            while let Some(i) = find(&data[from..], marker.as_bytes()) {
                let i = from + i;
                if i == 0 || !data[i - 1].is_ascii_digit() {
                    found = Some(i + marker.len());
                }
                from = i + 1;
            }
            let start = skip_ws(data, found?);
            &data[start..]
        };
        let entries = parse_dict(dict)?;
        let get = |key: &str| entries.iter().find(|(k, _)| k == key).map(|(_, v)| v);
        let int = |key: &str| {
            get(key).and_then(|v| std::str::from_utf8(v).ok()?.trim().parse::<i64>().ok())
        };
        if get("Filter").map(|v| v.as_slice()) != Some(&b"/Standard"[..]) {
            return None;
        }
        let revision = int("R")? as u8;
        let version = int("V").unwrap_or(0);
        let key_len = match version {
            1 => 5,
            2 => int("Length").unwrap_or(40) as usize / 8,
            4 => int("Length").unwrap_or(128) as usize / 8,
            _ => 32,
        }
        .max(5);
        Some(Self {
            revision,
            key_len,
            owner: decode_string(get("O")?)?,
            user: decode_string(get("U")?)?,
        })
    }

    /// The user password `password` opens the file with: itself, or the one
    /// an owner password recovers (revisions 2–4).
    fn user_password(&self, bytes: &Arc<Vec<u8>>, password: &str) -> Option<String> {
        if Pdf::new_with_password(bytes.clone(), password).is_ok() {
            return Some(password.to_string());
        }
        let user = self.recover_user(password.as_bytes())?;
        Pdf::new_with_password(bytes.clone(), &user)
            .is_ok()
            .then_some(user)
    }

    fn is_user(&self, bytes: &Arc<Vec<u8>>, password: &str) -> bool {
        if self.revision >= 5 {
            return self.user.len() >= 40
                && self
                    .hash_r56(password.as_bytes(), &self.user[32..40], &[])
                    .is_some_and(|h| h[..] == self.user[..32]);
        }
        Pdf::new_with_password(bytes.clone(), password).is_ok()
    }

    fn is_owner(&self, bytes: &Arc<Vec<u8>>, password: &str) -> bool {
        if self.revision >= 5 {
            if self.owner.len() < 40 || self.user.len() < 48 {
                return false;
            }
            return self
                .hash_r56(password.as_bytes(), &self.owner[32..40], &self.user[..48])
                .is_some_and(|h| h[..] == self.owner[..32]);
        }
        self.recover_user(password.as_bytes())
            .is_some_and(|user| Pdf::new_with_password(bytes.clone(), &user).is_ok())
    }

    /// Algorithm 7: the padded user password the owner password decrypts
    /// from `/O`, without its padding.
    fn recover_user(&self, owner: &[u8]) -> Option<String> {
        use md5::{Digest, Md5};
        if self.revision >= 5 || self.owner.len() < 32 {
            return None;
        }
        let mut hash = Md5::digest(pad(owner)).to_vec();
        if self.revision >= 3 {
            for _ in 0..50 {
                hash = Md5::digest(&hash).to_vec();
            }
        }
        let key = &hash[..self.key_len.min(16)];
        let mut user = self.owner[..32].to_vec();
        if self.revision == 2 {
            user = rc4(key, &user);
        } else {
            for i in (0..=19u8).rev() {
                let round: Vec<u8> = key.iter().map(|b| b ^ i).collect();
                user = rc4(&round, &user);
            }
        }
        let len = (0..=32).find(|&n| user[n..] == PADDING[..32 - n])?;
        // ponytail: a user password that is not UTF-8 cannot be handed to the
        // reader, which takes `&str`.
        String::from_utf8(user[..len].to_vec()).ok()
    }

    /// Revision 5 (SHA-256) or 6 (algorithm 2.B) password hash.
    fn hash_r56(&self, password: &[u8], salt: &[u8], user_key: &[u8]) -> Option<[u8; 32]> {
        use aes::cipher::{BlockEncrypt, KeyInit};
        use sha2::{Digest, Sha256, Sha384, Sha512};
        let password = &password[..password.len().min(127)];
        let mut k: Vec<u8> = Sha256::new()
            .chain_update(password)
            .chain_update(salt)
            .chain_update(user_key)
            .finalize()
            .to_vec();
        if self.revision == 6 {
            let mut round = 0u32;
            let mut last = 0u8;
            while round < 64 || u32::from(last) > round - 32 {
                let mut k1 = Vec::with_capacity(64 * (password.len() + k.len() + user_key.len()));
                for _ in 0..64 {
                    k1.extend_from_slice(password);
                    k1.extend_from_slice(&k);
                    k1.extend_from_slice(user_key);
                }
                let cipher = aes::Aes128::new_from_slice(&k[..16]).ok()?;
                let mut iv: [u8; 16] = k[16..32].try_into().ok()?;
                for block in k1.chunks_exact_mut(16) {
                    for (b, v) in block.iter_mut().zip(iv) {
                        *b ^= v;
                    }
                    cipher.encrypt_block(block.into());
                    iv.copy_from_slice(block);
                }
                let modulo = k1[..16].iter().map(|b| u32::from(*b)).sum::<u32>() % 3;
                k = match modulo {
                    0 => Sha256::digest(&k1).to_vec(),
                    1 => Sha384::digest(&k1).to_vec(),
                    _ => Sha512::digest(&k1).to_vec(),
                };
                last = *k1.last()?;
                round += 1;
            }
        }
        k[..32].try_into().ok()
    }
}

fn pad(password: &[u8]) -> [u8; 32] {
    let mut out = PADDING;
    let n = password.len().min(32);
    out[..n].copy_from_slice(&password[..n]);
    out[n..].copy_from_slice(&PADDING[..32 - n]);
    out
}

fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut s: [u8; 256] = std::array::from_fn(|i| i as u8);
    let mut j = 0u8;
    for i in 0..256 {
        j = j.wrapping_add(s[i]).wrapping_add(key[i % key.len()]);
        s.swap(i, j as usize);
    }
    let (mut i, mut j) = (0u8, 0u8);
    data.iter()
        .map(|b| {
            i = i.wrapping_add(1);
            j = j.wrapping_add(s[i as usize]);
            s.swap(i as usize, j as usize);
            b ^ s[s[i as usize].wrapping_add(s[j as usize]) as usize]
        })
        .collect()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Whether the file declares an encryption dictionary.
pub(crate) fn is_encrypted(data: &[u8]) -> bool {
    rfind_key(data, b"/Encrypt").is_some()
}

/// The last `key` name that is not the start of a longer name
/// (`/Encrypt`, not `/EncryptMetadata`).
fn rfind_key(data: &[u8], key: &[u8]) -> Option<usize> {
    (0..data.len().saturating_sub(key.len())).rev().find(|&i| {
        data[i..].starts_with(key)
            && !data
                .get(i + key.len())
                .is_some_and(|c| c.is_ascii_alphanumeric())
    })
}

fn skip_ws(data: &[u8], mut i: usize) -> usize {
    while data.get(i).is_some_and(|c| c.is_ascii_whitespace()) {
        i += 1;
    }
    i
}

fn read_int(data: &[u8], i: usize) -> Option<(u64, usize)> {
    let end = i + data[i..].iter().take_while(|c| c.is_ascii_digit()).count();
    Some((std::str::from_utf8(&data[i..end]).ok()?.parse().ok()?, end))
}

/// The top-level `key value` pairs of the dictionary `data` starts with;
/// values are kept as their source bytes.
fn parse_dict(data: &[u8]) -> Option<Vec<(String, Vec<u8>)>> {
    if !data.starts_with(b"<<") {
        return None;
    }
    let mut i = 2;
    let mut out = Vec::new();
    loop {
        i = skip_ws(data, i);
        if data[i..].starts_with(b">>") {
            return Some(out);
        }
        if *data.get(i)? != b'/' {
            return None;
        }
        let start = i + 1;
        i = start;
        while data
            .get(i)
            .is_some_and(|c| !c.is_ascii_whitespace() && !b"/<>[]()".contains(c))
        {
            i += 1;
        }
        let key = String::from_utf8_lossy(&data[start..i]).into_owned();
        i = skip_ws(data, i);
        let end = value_end(data, i)?;
        out.push((key, data[i..end].to_vec()));
        i = end;
    }
}

/// Where the value starting at `i` ends.
fn value_end(data: &[u8], i: usize) -> Option<usize> {
    match *data.get(i)? {
        b'(' => {
            let (mut depth, mut j) = (0, i);
            loop {
                match *data.get(j)? {
                    b'\\' => j += 1,
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(j + 1);
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
        }
        b'<' if data.get(i + 1) == Some(&b'<') => {
            let (mut depth, mut j) = (0, i);
            loop {
                if data[j..].starts_with(b"<<") {
                    depth += 1;
                    j += 2;
                } else if data[j..].starts_with(b">>") {
                    depth -= 1;
                    j += 2;
                    if depth == 0 {
                        return Some(j);
                    }
                } else if data[j] == b'(' {
                    j = value_end(data, j)?;
                } else {
                    j += 1;
                }
                data.get(j)?;
            }
        }
        b'<' => Some(i + find(&data[i..], b">")? + 1),
        b'[' => {
            let mut j = i + 1;
            loop {
                j = skip_ws(data, j);
                if *data.get(j)? == b']' {
                    return Some(j + 1);
                }
                j = value_end(data, j)?;
            }
        }
        b'/' => {
            let mut j = i + 1;
            while data
                .get(j)
                .is_some_and(|c| !c.is_ascii_whitespace() && !b"/<>[]()".contains(c))
            {
                j += 1;
            }
            Some(j)
        }
        _ => {
            // A number, `true`/`false`, or a reference `n g R`.
            let mut j = i;
            while data
                .get(j)
                .is_some_and(|c| !c.is_ascii_whitespace() && !b"/<>[]()".contains(c))
            {
                j += 1;
            }
            let save = j;
            let reference = read_int(data, skip_ws(data, j)).filter(|_| data[i].is_ascii_digit());
            if let Some((_, after)) = reference {
                let r = skip_ws(data, after);
                if data.get(r) == Some(&b'R') {
                    return Some(r + 1);
                }
            }
            Some(save)
        }
    }
}

/// A literal `( … )` or hex `< … >` string's bytes.
fn decode_string(raw: &[u8]) -> Option<Vec<u8>> {
    match raw.first()? {
        b'<' => {
            let digits: Vec<u8> = raw[1..raw.len() - 1]
                .iter()
                .filter(|c| c.is_ascii_hexdigit())
                .map(|c| (*c as char).to_digit(16).unwrap_or(0) as u8)
                .collect();
            Some(
                digits
                    .chunks(2)
                    .map(|p| (p[0] << 4) | p.get(1).copied().unwrap_or(0))
                    .collect(),
            )
        }
        b'(' => {
            let body = &raw[1..raw.len() - 1];
            let mut out = Vec::with_capacity(body.len());
            let mut i = 0;
            while i < body.len() {
                let c = body[i];
                i += 1;
                if c != b'\\' {
                    out.push(c);
                    continue;
                }
                let Some(&e) = body.get(i) else { break };
                i += 1;
                match e {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'\r' => {
                        if body.get(i) == Some(&b'\n') {
                            i += 1;
                        }
                    }
                    b'\n' => {}
                    b'0'..=b'7' => {
                        let mut v = u32::from(e - b'0');
                        for _ in 0..2 {
                            match body.get(i) {
                                Some(d @ b'0'..=b'7') => {
                                    v = v * 8 + u32::from(d - b'0');
                                    i += 1;
                                }
                                _ => break,
                            }
                        }
                        out.push(v as u8);
                    }
                    other => out.push(other),
                }
            }
            Some(out)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The revision-2 file `mkpdf` writes: user `test`, owner `owner`.
    fn sample() -> Security {
        Security {
            revision: 2,
            key_len: 5,
            owner: decode_string(
                b"<95fe1f4219662a774442fb072e3d9f19e9d130ec09a4d0061e78fe920f7ab62f>",
            )
            .unwrap(),
            user: Vec::new(),
        }
    }

    #[test]
    fn owner_password_recovers_user_password() {
        assert_eq!(sample().recover_user(b"owner").as_deref(), Some("test"));
        assert_ne!(sample().recover_user(b"test").as_deref(), Some("test"));
    }

    #[test]
    fn reads_encrypt_dictionary() {
        let data = b"5 0 obj\n<< /Filter /Standard /V 1 /R 2 /O (a\\)b) /U <0102> /P -44 >>\n\
            endobj\ntrailer\n<< /Root 1 0 R /Encrypt 5 0 R >>";
        let s = Security::read(data).unwrap();
        assert_eq!((s.revision, s.key_len), (2, 5));
        assert_eq!(s.owner, b"a)b");
        assert_eq!(s.user, [1, 2]);
    }
}
