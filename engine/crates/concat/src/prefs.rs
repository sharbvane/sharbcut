// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer and Concat contributors

//! What the window remembers between runs, as one small JSON file in the
//! app's config directory. None of it is project state: the theme, which
//! models are chosen, which languages. A missing or unreadable file is the
//! defaults, never an error.

use concat_host::AppDirs;
use serde::{Deserialize, Serialize};

const FILE: &str = "settings.json";

/// Remembered preferences.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Preferences {
    /// The dark theme. `None` is the app's default, which is dark.
    pub dark: Option<bool>,
    /// The chosen transcriber model id, e.g. "base.en".
    pub transcriber_model: Option<String>,
    /// The chosen speech model id.
    pub tts_model: Option<String>,
    /// The chosen Kokoro speaker id.
    pub tts_voice: Option<i32>,
    /// The interface's locale code ("de", "pt-BR", ...); absent is Simplified Chinese.
    pub locale: Option<String>,
    /// Package ids starred in the effect libraries, in no order. One list
    /// across all three shelves: a star is a fact about a package, and which
    /// library it happens to be filed in is not part of it.
    #[serde(default)]
    pub favourites: Vec<String>,
    /// The playhead stops at the end of the content instead of going where
    /// it is put. Off by default: a click past the last clip lands there, so
    /// a clip can be dropped at the playhead beyond everything else.
    pub playhead_stops_at_end: bool,
    /// OpenAI-compatible endpoint; the secret is kept in OS credentials.
    pub ai_base_url: Option<String>,
    pub ai_model: Option<String>,
    pub ai_reasoning_effort: Option<String>,
}

#[cfg(windows)]
const AI_CREDENTIAL: &str = "SharbCut/AI-Agent";

#[cfg(windows)]
fn credential_name() -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(AI_CREDENTIAL)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// The user's own API key never enters `settings.json` or a project file.
#[cfg(windows)]
pub fn load_api_key() -> Result<Option<String>, String> {
    use windows_sys::Win32::Security::Credentials::{
        CRED_TYPE_GENERIC, CREDENTIALW, CredFree, CredReadW,
    };

    let name = credential_name();
    let mut pointer: *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: name is NUL-terminated; CredReadW owns the returned allocation.
    if unsafe { CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut pointer) } == 0 {
        let error = std::io::Error::last_os_error();
        return if error.raw_os_error() == Some(1168) {
            Ok(None)
        } else {
            Err(format!("Cannot read saved AI key: {error}"))
        };
    }
    // SAFETY: a successful CredReadW returns a valid credential and blob.
    let bytes = if unsafe { (*pointer).CredentialBlobSize } == 0 {
        Vec::new()
    } else {
        // SAFETY: the non-empty blob belongs to the returned credential.
        unsafe {
            std::slice::from_raw_parts(
                (*pointer).CredentialBlob,
                (*pointer).CredentialBlobSize as usize,
            )
            .to_vec()
        }
    };
    let result = String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| format!("Saved AI key is invalid UTF-8: {error}"));
    // SAFETY: pointer was allocated by CredReadW.
    unsafe { CredFree(pointer.cast()) };
    result
}

#[cfg(windows)]
pub fn save_api_key(key: &str) -> Result<(), String> {
    use windows_sys::Win32::Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredWriteW,
    };

    let mut name = credential_name();
    if key.is_empty() {
        // SAFETY: name is NUL-terminated.
        if unsafe { CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(1168) {
                return Err(format!("Cannot clear saved AI key: {error}"));
            }
        }
        return Ok(());
    }
    if key.len() > 2048 {
        return Err("AI key is too long.".to_owned());
    }
    let mut user: Vec<u16> = "SharbCut\0".encode_utf16().collect();
    let mut blob = key.as_bytes().to_vec();
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: name.as_mut_ptr(),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: user.as_mut_ptr(),
        ..Default::default()
    };
    // SAFETY: all pointers remain live until CredWriteW copies the fields.
    if unsafe { CredWriteW(&credential, 0) } == 0 {
        return Err(format!(
            "Cannot save AI key: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn load_api_key() -> Result<Option<String>, String> {
    Ok(None)
}

#[cfg(not(windows))]
pub fn save_api_key(_key: &str) -> Result<(), String> {
    Err("Secure AI key storage currently requires Windows.".to_owned())
}

impl Preferences {
    /// Reads the file, or the defaults when there is none.
    pub fn load(dirs: &AppDirs) -> Self {
        std::fs::read(dirs.config.join(FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Writes the file. Best effort: a preference that did not stick is
    /// not worth interrupting anyone over.
    pub fn save(&self, dirs: &AppDirs) {
        let _ = std::fs::create_dir_all(&dirs.config);
        if let Ok(encoded) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(dirs.config.join(FILE), encoded);
        }
    }
}
