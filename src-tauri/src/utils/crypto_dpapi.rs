use base64::{engine::general_purpose::STANDARD, Engine};

const DPAPI_PREFIX: &str = "dpapi:";
const ELECTRON_ENC_PREFIX: &str = "enc:";

pub fn is_encrypted(text: &str) -> bool {
    text.starts_with(DPAPI_PREFIX) || text.starts_with(ELECTRON_ENC_PREFIX)
}

#[cfg(windows)]
pub fn encrypt_string(plain: &str) -> String {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};

    if plain.is_empty() || plain.starts_with(DPAPI_PREFIX) {
        return plain.to_string();
    }

    let bytes = plain.as_bytes();
    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };

    let ok = unsafe {
        CryptProtectData(
            &in_blob,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            0,
            &mut out_blob,
        )
    };
    if ok == 0 || out_blob.pbData.is_null() {
        return plain.to_string();
    }

    let protected = unsafe {
        let slice = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize);
        let encoded = STANDARD.encode(slice);
        LocalFree(out_blob.pbData as _);
        encoded
    };
    format!("{DPAPI_PREFIX}{protected}")
}

#[cfg(not(windows))]
pub fn encrypt_string(plain: &str) -> String {
    plain.to_string()
}

#[cfg(windows)]
pub fn decrypt_string(cipher: &str) -> String {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};

    if cipher.is_empty() {
        return String::new();
    }
    if cipher.starts_with(ELECTRON_ENC_PREFIX) {
        // Electron Chromium AES-GCM safeStorage key cannot be unwrapped directly without Chromium's Local State key;
        // return empty string cleanly so KeyManager regenerates a service key only if service mode is initialized.
        return String::new();
    }
    if !cipher.starts_with(DPAPI_PREFIX) {
        return cipher.to_string();
    }

    let b64 = &cipher[DPAPI_PREFIX.len()..];
    let Ok(raw) = STANDARD.decode(b64) else {
        return String::new();
    };
    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: raw.len() as u32,
        pbData: raw.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };
    let ok = unsafe {
        CryptUnprotectData(
            &in_blob,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            0,
            &mut out_blob,
        )
    };
    if ok == 0 || out_blob.pbData.is_null() {
        return String::new();
    }
    unsafe {
        let slice = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize);
        let text = String::from_utf8_lossy(slice).to_string();
        LocalFree(out_blob.pbData as _);
        text
    }
}

#[cfg(not(windows))]
pub fn decrypt_string(cipher: &str) -> String {
    cipher.to_string()
}
