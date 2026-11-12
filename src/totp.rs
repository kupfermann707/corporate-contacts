use std::time::{SystemTime, UNIX_EPOCH};

const BASE32_ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// Computes SHA-1 hash (FIPS 180-1)
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x6745_2301;
    let mut h1: u32 = 0xefcd_ab89;
    let mut h2: u32 = 0x98ba_dcfe;
    let mut h3: u32 = 0x1032_5476;
    let mut h4: u32 = 0xc3d2_e1f0;

    let bit_len = (data.len() as u64) * 8;
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0x00);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;

        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5a82_7999),
                20..=39 => (b ^ c ^ d, 0x6ed9_eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
                _ => (b ^ c ^ d, 0xca62_c1d6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0u8; 20];
    out[0..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

/// Computes HMAC-SHA1
pub fn hmac_sha1(key: &[u8], message: &[u8]) -> [u8; 20] {
    let mut key_buf = [0u8; 64];
    if key.len() > 64 {
        let hash = sha1(key);
        key_buf[..20].copy_from_slice(&hash);
    } else {
        key_buf[..key.len()].copy_from_slice(key);
    }

    let mut inner_input = Vec::with_capacity(64 + message.len());
    for &b in &key_buf {
        inner_input.push(b ^ 0x36);
    }
    inner_input.extend_from_slice(message);
    let inner_hash = sha1(&inner_input);

    let mut outer_input = Vec::with_capacity(64 + 20);
    for &b in &key_buf {
        outer_input.push(b ^ 0x5c);
    }
    outer_input.extend_from_slice(&inner_hash);
    sha1(&outer_input)
}

/// Encodes raw bytes to RFC 4648 Base32 string
pub fn encode_base32(data: &[u8]) -> String {
    let mut result = String::new();
    let mut buffer = 0u64;
    let mut bits_in_buffer = 0;

    for &byte in data {
        buffer = (buffer << 8) | (byte as u64);
        bits_in_buffer += 8;
        while bits_in_buffer >= 5 {
            bits_in_buffer -= 5;
            let index = ((buffer >> bits_in_buffer) & 0x1f) as usize;
            result.push(BASE32_ALPHABET[index] as char);
        }
    }

    if bits_in_buffer > 0 {
        let index = ((buffer << (5 - bits_in_buffer)) & 0x1f) as usize;
        result.push(BASE32_ALPHABET[index] as char);
    }

    result
}

/// Decodes RFC 4648 Base32 string to raw bytes
pub fn decode_base32(input: &str) -> Result<Vec<u8>, String> {
    let clean: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '=')
        .collect();

    let mut result = Vec::new();
    let mut buffer = 0u64;
    let mut bits_in_buffer = 0;

    for ch in clean.chars() {
        let upper = ch.to_ascii_uppercase();
        let val = match upper {
            'A'..='Z' => (upper as u8 - b'A') as u64,
            '2'..='7' => (upper as u8 - b'2' + 26) as u64,
            _ => return Err(format!("Недопустимый символ в ключе Base32: '{}'", ch)),
        };

        buffer = (buffer << 5) | val;
        bits_in_buffer += 5;

        while bits_in_buffer >= 8 {
            bits_in_buffer -= 8;
            result.push(((buffer >> bits_in_buffer) & 0xff) as u8);
        }
    }

    Ok(result)
}

/// Generates a new random 20-byte Base32 secret for TOTP
pub fn generate_totp_secret() -> String {
    let mut bytes = [0u8; 20];
    crate::crypto::get_random_bytes(&mut bytes);
    encode_base32(&bytes)
}

/// Generates standard otpauth URI for QR codes or authenticator imports
pub fn format_totp_uri(secret: &str, account: &str, issuer: &str) -> String {
    format!(
        "otpauth://totp/{}:{}?secret={}&issuer={}&algorithm=SHA1&digits=6&period=30",
        issuer, account, secret, issuer
    )
}

/// Computes 6-digit TOTP code for a specific timestamp (RFC 6238)
pub fn compute_totp_code(secret_bytes: &[u8], timestamp: u64, digits: u32) -> String {
    let step = timestamp / 30;
    let step_bytes = step.to_be_bytes();
    let hash = hmac_sha1(secret_bytes, &step_bytes);

    let offset = (hash[19] & 0x0f) as usize;
    let binary = (((hash[offset] & 0x7f) as u32) << 24)
        | ((hash[offset + 1] as u32) << 16)
        | ((hash[offset + 2] as u32) << 8)
        | (hash[offset + 3] as u32);

    let modulus = 10u32.pow(digits);
    let otp = binary % modulus;
    format!("{:0width$}", otp, width = digits as usize)
}

/// Verifies a 6-digit TOTP code against current time with ±30s window
pub fn verify_totp(secret: &str, code: &str) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let secret_bytes = match decode_base32(secret) {
        Ok(b) => b,
        Err(_) => return false,
    };

    let clean_code = code.trim();
    if clean_code.len() != 6 || !clean_code.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }

    let steps = [now.saturating_sub(30), now, now.saturating_add(30)];
    for t in steps {
        if compute_totp_code(&secret_bytes, t, 6) == clean_code {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rfc6238_hmac_sha1_vectors() {
        // RFC 6238 Appendix B test vector for SHA1:
        // Secret: "12345678901234567890" (ASCII)
        let secret = b"12345678901234567890";

        assert_eq!(compute_totp_code(secret, 59, 8), "94287082");
        assert_eq!(compute_totp_code(secret, 59, 6), "287082");

        assert_eq!(compute_totp_code(secret, 1111111109, 8), "07081804");
        assert_eq!(compute_totp_code(secret, 1111111109, 6), "081804");

        assert_eq!(compute_totp_code(secret, 1111111111, 8), "14050471");
        assert_eq!(compute_totp_code(secret, 1111111111, 6), "050471");

        assert_eq!(compute_totp_code(secret, 1234567890, 8), "89005924");
        assert_eq!(compute_totp_code(secret, 1234567890, 6), "005924");

        assert_eq!(compute_totp_code(secret, 2000000000, 8), "69279037");
        assert_eq!(compute_totp_code(secret, 2000000000, 6), "279037");
    }

    #[test]
    fn test_base32_roundtrip() {
        let raw = b"HelloWorld12345!";
        let encoded = encode_base32(raw);
        let decoded = decode_base32(&encoded).expect("Decode base32 failed");
        assert_eq!(decoded, raw);
    }

    #[test]
    fn test_totp_verification() {
        let secret = generate_totp_secret();
        let secret_bytes = decode_base32(&secret).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let current_code = compute_totp_code(&secret_bytes, now, 6);
        assert!(verify_totp(&secret, &current_code));

        // Invalid code should fail
        assert!(!verify_totp(&secret, "000000") || current_code == "000000");
        assert!(!verify_totp(&secret, "invalid"));
    }
}
