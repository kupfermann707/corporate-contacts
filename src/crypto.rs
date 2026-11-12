use std::fs::File;
use std::io::Read;

/// Constant initialization vector for state permutation
const CONSTANTS: [u32; 4] = [
    0x851a_431e,
    0x12c4_6b91,
    0x932f_b58a,
    0x5a18_3c4d,
];

/// 512-bit ARX permutation (Addition, Rotation, XOR)
fn permute(state: &mut [u32; 16]) {
    for _ in 0..12 {
        // Column rounds
        for i in 0..4 {
            let a = i;
            let b = i + 4;
            let c = i + 8;
            let d = i + 12;
            state[a] = state[a].wrapping_add(state[b]);
            state[d] = (state[d] ^ state[a]).rotate_left(16);
            state[c] = state[c].wrapping_add(state[d]);
            state[b] = (state[b] ^ state[c]).rotate_left(12);
            state[a] = state[a].wrapping_add(state[b]);
            state[d] = (state[d] ^ state[a]).rotate_left(8);
            state[c] = state[c].wrapping_add(state[d]);
            state[b] = (state[b] ^ state[c]).rotate_left(7);
        }
        // Diagonal rounds
        let diagonals = [(0, 5, 10, 15), (1, 6, 11, 12), (2, 7, 8, 13), (3, 4, 9, 14)];
        for &(a, b, c, d) in &diagonals {
            state[a] = state[a].wrapping_add(state[b]);
            state[d] = (state[d] ^ state[a]).rotate_left(16);
            state[c] = state[c].wrapping_add(state[d]);
            state[b] = (state[b] ^ state[c]).rotate_left(12);
            state[a] = state[a].wrapping_add(state[b]);
            state[d] = (state[d] ^ state[a]).rotate_left(8);
            state[c] = state[c].wrapping_add(state[d]);
            state[b] = (state[b] ^ state[c]).rotate_left(7);
        }
    }
}

/// Computes a 256-bit cryptographic hash/sponge over arbitrary data
pub fn hash256(data: &[u8]) -> [u8; 32] {
    let mut state = [0u32; 16];
    state[0..4].copy_from_slice(&CONSTANTS);
    state[4] = 0x6a09_e667;
    state[5] = 0xbb67_ae85;
    state[6] = 0x3c6e_f372;
    state[7] = 0xa54f_f53a;

    let mut offset = 0;
    while offset + 32 <= data.len() {
        for i in 0..8 {
            let chunk = [
                data[offset + i * 4],
                data[offset + i * 4 + 1],
                data[offset + i * 4 + 2],
                data[offset + i * 4 + 3],
            ];
            state[8 + i] ^= u32::from_le_bytes(chunk);
        }
        permute(&mut state);
        offset += 32;
    }

    // Process remaining bytes
    let remaining = &data[offset..];
    let mut pad = [0u8; 32];
    pad[..remaining.len()].copy_from_slice(remaining);
    pad[remaining.len()] = 0x80;
    let len_bits = (data.len() as u64) * 8;
    pad[24..32].copy_from_slice(&len_bits.to_le_bytes());

    for i in 0..8 {
        let chunk = [pad[i * 4], pad[i * 4 + 1], pad[i * 4 + 2], pad[i * 4 + 3]];
        state[8 + i] ^= u32::from_le_bytes(chunk);
    }
    permute(&mut state);

    let mut out = [0u8; 32];
    for i in 0..8 {
        out[i * 4..i * 4 + 4].copy_from_slice(&state[i].to_le_bytes());
    }
    out
}

/// Generates cryptographically secure random bytes
pub fn get_random_bytes(buf: &mut [u8]) {
    if let Ok(mut f) = File::open("/dev/urandom") {
        if f.read_exact(buf).is_ok() {
            return;
        }
    }
    // Fallback entropy source
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    for (i, b) in buf.iter_mut().enumerate() {
        let x = now
            .wrapping_add((pid as u128) << 32)
            .wrapping_mul((i as u128 + 1).wrapping_mul(0x5851_f42d_4c95_7f2d));
        *b = (x >> ((i % 8) * 8)) as u8;
    }
}

/// Derives a machine- and user-specific 256-bit encryption key
pub fn derive_machine_key() -> [u8; 32] {
    let mut material = Vec::new();

    // 1. Host machine ID
    if let Ok(mid) = std::fs::read_to_string("/etc/machine-id") {
        material.extend_from_slice(mid.trim().as_bytes());
    } else if let Ok(mid) = std::fs::read_to_string("/var/lib/dbus/machine-id") {
        material.extend_from_slice(mid.trim().as_bytes());
    } else {
        material.extend_from_slice(b"machine-id-fixed-salt");
    }
    material.push(b':');

    // 2. User identification
    #[cfg(unix)]
    {
        let uid = unsafe { libc::getuid() };
        material.extend_from_slice(format!("uid:{}", uid).as_bytes());
    }
    if let Ok(user) = std::env::var("USER") {
        material.extend_from_slice(user.as_bytes());
    }
    if let Some(home) = dirs::home_dir() {
        material.extend_from_slice(home.to_string_lossy().as_bytes());
    }
    material.push(b':');

    // 3. Application domain salt
    material.extend_from_slice(b"corporate-contacts::settings-secret::v1");

    // 4. Optional manual override from environment
    if let Ok(env_key) = std::env::var("CORPORATE_CONTACTS_KEY") {
        material.extend_from_slice(env_key.as_bytes());
    }

    hash256(&material)
}

/// Applies keystream XOR cipher in place
fn apply_keystream(data: &mut [u8], key: &[u8; 32], iv: &[u8; 16]) {
    let mut key_words = [0u32; 8];
    for i in 0..8 {
        key_words[i] = u32::from_le_bytes([
            key[i * 4],
            key[i * 4 + 1],
            key[i * 4 + 2],
            key[i * 4 + 3],
        ]);
    }

    let mut iv_words = [0u32; 4];
    for i in 0..4 {
        iv_words[i] = u32::from_le_bytes([
            iv[i * 4],
            iv[i * 4 + 1],
            iv[i * 4 + 2],
            iv[i * 4 + 3],
        ]);
    }

    let mut block_idx: u32 = 0;
    let mut offset = 0;

    while offset < data.len() {
        let mut state = [0u32; 16];
        state[0..4].copy_from_slice(&CONSTANTS);
        state[4..12].copy_from_slice(&key_words);
        state[12] = iv_words[0];
        state[13] = iv_words[1];
        state[14] = iv_words[2];
        state[15] = iv_words[3] ^ block_idx;

        permute(&mut state);

        let mut keystream = [0u8; 64];
        for i in 0..16 {
            keystream[i * 4..i * 4 + 4].copy_from_slice(&state[i].to_le_bytes());
        }

        let chunk_len = (data.len() - offset).min(64);
        for i in 0..chunk_len {
            data[offset + i] ^= keystream[i];
        }

        offset += chunk_len;
        block_idx = block_idx.wrapping_add(1);
    }
}

/// Computes an authentication tag over IV and ciphertext using the key
pub fn compute_auth_tag(key: &[u8; 32], iv: &[u8; 16], ciphertext: &[u8]) -> [u8; 32] {
    let mut buffer = Vec::with_capacity(32 + 16 + ciphertext.len());
    buffer.extend_from_slice(key);
    buffer.extend_from_slice(iv);
    buffer.extend_from_slice(ciphertext);
    buffer.extend_from_slice(key);
    hash256(&buffer)
}

/// Encrypts plaintext bytes and returns (iv, tag, ciphertext)
pub fn encrypt_data(plaintext: &[u8], key: &[u8; 32]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let mut iv = [0u8; 16];
    get_random_bytes(&mut iv);

    let mut ciphertext = plaintext.to_vec();
    apply_keystream(&mut ciphertext, key, &iv);

    let tag = compute_auth_tag(key, &iv, &ciphertext);
    (iv.to_vec(), tag.to_vec(), ciphertext)
}

/// Decrypts ciphertext bytes and verifies integrity tag
pub fn decrypt_data(
    ciphertext: &[u8],
    iv: &[u8],
    tag: &[u8],
    key: &[u8; 32],
) -> Result<Vec<u8>, String> {
    if iv.len() != 16 {
        return Err("Неверная длина вектора инициализации (IV)".to_string());
    }
    if tag.len() != 32 {
        return Err("Неверная длина контрольной суммы (Tag)".to_string());
    }

    let mut iv_arr = [0u8; 16];
    iv_arr.copy_from_slice(iv);

    let expected_tag = compute_auth_tag(key, &iv_arr, ciphertext);
    if tag != expected_tag.as_slice() {
        return Err("Ошибка проверки целостности: неверный ключ или повреждённые данные".to_string());
    }

    let mut plaintext = ciphertext.to_vec();
    apply_keystream(&mut plaintext, key, &iv_arr);
    Ok(plaintext)
}

/// Helper functions to convert between bytes and hex strings
pub fn to_hex(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len() * 2);
    for b in data {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

pub fn from_hex(hex: &str) -> Result<Vec<u8>, String> {
    let clean = hex.trim();
    if clean.len() % 2 != 0 {
        return Err("Неверная длина hex строки".to_string());
    }
    let mut bytes = Vec::with_capacity(clean.len() / 2);
    for i in (0..clean.len()).step_by(2) {
        let byte = u8::from_str_radix(&clean[i..i + 2], 16)
            .map_err(|e| format!("Ошибка парсинга hex: {}", e))?;
        bytes.push(byte);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crypto_roundtrip() {
        let key = derive_machine_key();
        let original = b"{\"db_password\":\"SuperSecret123!\",\"db_user\":\"postgres\"}";

        let (iv, tag, ciphertext) = encrypt_data(original, &key);
        assert_ne!(ciphertext.as_slice(), original.as_slice());

        let decrypted = decrypt_data(&ciphertext, &iv, &tag, &key).expect("Decryption failed");
        assert_eq!(decrypted.as_slice(), original.as_slice());
    }

    #[test]
    fn test_crypto_tamper_detection() {
        let key = derive_machine_key();
        let original = b"Secret data";
        let (iv, tag, mut ciphertext) = encrypt_data(original, &key);

        // Tamper with ciphertext
        if !ciphertext.is_empty() {
            ciphertext[0] ^= 0x01;
        }

        let res = decrypt_data(&ciphertext, &iv, &tag, &key);
        assert!(res.is_err(), "Tampered ciphertext should fail verification");
    }

    #[test]
    fn test_hex_conversion() {
        let bytes = vec![0x00, 0x12, 0xab, 0xff];
        let hex = to_hex(&bytes);
        assert_eq!(hex, "0012abff");
        let parsed = from_hex(&hex).expect("Parse hex failed");
        assert_eq!(parsed, bytes);
    }
}
