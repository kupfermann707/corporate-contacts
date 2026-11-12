/// Lightweight, pure-Rust QR code generator (ISO/IEC 18004)
/// Specifically optimized for TOTP URI payloads (versions 1 through 8, Level L)

const EXP_TABLE: [u8; 512] = {
    let mut exp = [0u8; 512];
    let mut val = 1u16;
    let mut i = 0;
    while i < 255 {
        exp[i] = val as u8;
        exp[i + 255] = val as u8;
        val <<= 1;
        if val >= 256 {
            val ^= 0x11d;
        }
        i += 1;
    }
    exp
};

const LOG_TABLE: [u8; 256] = {
    let mut log = [0u8; 256];
    let mut val = 1u16;
    let mut i = 0;
    while i < 255 {
        log[val as usize] = i as u8;
        val <<= 1;
        if val >= 256 {
            val ^= 0x11d;
        }
        i += 1;
    }
    log
};

fn gf_mul(a: u8, b: u8) -> u8 {
    if a == 0 || b == 0 {
        0
    } else {
        EXP_TABLE[(LOG_TABLE[a as usize] as usize) + (LOG_TABLE[b as usize] as usize)]
    }
}

fn rs_generator(k: usize) -> Vec<u8> {
    let mut g = vec![1u8];
    for i in 0..k {
        let mut next = vec![0u8; g.len() + 1];
        let factor = EXP_TABLE[i];
        for (j, &c) in g.iter().enumerate() {
            next[j] ^= c;
            next[j + 1] ^= gf_mul(c, factor);
        }
        g = next;
    }
    g
}

fn rs_encode(msg: &[u8], ec_len: usize) -> Vec<u8> {
    let gen = rs_generator(ec_len);
    let mut remainder = vec![0u8; ec_len];
    for &b in msg {
        let lead = b ^ remainder[0];
        remainder.copy_within(1..ec_len, 0);
        remainder[ec_len - 1] = 0;
        if lead != 0 {
            for j in 0..ec_len {
                remainder[j] ^= gf_mul(lead, gen[j + 1]);
            }
        }
    }
    remainder
}

#[derive(Clone, Copy)]
struct QrVersionSpec {
    version: usize,
    data_codewords: usize,
    ec_codewords_per_block: usize,
    blocks: usize,
    alignment_coords: &'static [usize],
}

const VERSION_SPECS: [QrVersionSpec; 8] = [
    // Version 1: 21x21
    QrVersionSpec { version: 1, data_codewords: 19, ec_codewords_per_block: 7, blocks: 1, alignment_coords: &[] },
    // Version 2: 25x25
    QrVersionSpec { version: 2, data_codewords: 34, ec_codewords_per_block: 10, blocks: 1, alignment_coords: &[6, 18] },
    // Version 3: 29x29
    QrVersionSpec { version: 3, data_codewords: 55, ec_codewords_per_block: 15, blocks: 1, alignment_coords: &[6, 22] },
    // Version 4: 33x33
    QrVersionSpec { version: 4, data_codewords: 80, ec_codewords_per_block: 20, blocks: 1, alignment_coords: &[6, 26] },
    // Version 5: 37x37
    QrVersionSpec { version: 5, data_codewords: 108, ec_codewords_per_block: 26, blocks: 1, alignment_coords: &[6, 30] },
    // Version 6: 41x41 (2 blocks of 68 data, 18 ec)
    QrVersionSpec { version: 6, data_codewords: 136, ec_codewords_per_block: 18, blocks: 2, alignment_coords: &[6, 34] },
    // Version 7: 45x45 (2 blocks of 78 data, 20 ec)
    QrVersionSpec { version: 7, data_codewords: 156, ec_codewords_per_block: 20, blocks: 2, alignment_coords: &[6, 22, 38] },
    // Version 8: 49x49 (2 blocks of 97 data, 24 ec)
    QrVersionSpec { version: 8, data_codewords: 194, ec_codewords_per_block: 24, blocks: 2, alignment_coords: &[6, 24, 42] },
];

pub fn encode_qr(text: &str) -> Result<Vec<Vec<bool>>, String> {
    let raw = text.as_bytes();
    let spec = VERSION_SPECS
        .iter()
        .find(|s| {
            // Byte mode: 4 bits mode + 8 bits length + payload bytes * 8 bits
            let required_bits = 4 + 8 + raw.len() * 8;
            required_bits <= s.data_codewords * 8
        })
        .ok_or_else(|| "Данные слишком длинные для генерации QR-кода".to_string())?;

    // 1. Bitstream assembly
    let mut bitstream = Vec::with_capacity(spec.data_codewords * 8);

    // Mode: Byte (0100)
    for &b in &[false, true, false, false] {
        bitstream.push(b);
    }

    // Character count indicator (8 bits for versions 1..=9)
    let count = raw.len() as u8;
    for i in (0..8).rev() {
        bitstream.push(((count >> i) & 1) == 1);
    }

    // Payload bytes
    for &byte in raw {
        for i in (0..8).rev() {
            bitstream.push(((byte >> i) & 1) == 1);
        }
    }

    // Terminator (up to 4 zero bits)
    let term_len = 4.min(spec.data_codewords * 8 - bitstream.len());
    for _ in 0..term_len {
        bitstream.push(false);
    }

    // Pad to byte boundary
    while (bitstream.len() % 8) != 0 {
        bitstream.push(false);
    }

    // Convert to bytes
    let mut data_codewords = Vec::with_capacity(spec.data_codewords);
    for chunk in bitstream.chunks_exact(8) {
        let mut b = 0u8;
        for (i, &bit) in chunk.iter().enumerate() {
            if bit {
                b |= 1 << (7 - i);
            }
        }
        data_codewords.push(b);
    }

    // Pad with alternating 0xEC, 0x11
    let mut pad_val = 0xECu8;
    while data_codewords.len() < spec.data_codewords {
        data_codewords.push(pad_val);
        pad_val = if pad_val == 0xEC { 0x11 } else { 0xEC };
    }

    // 2. Block division and Reed-Solomon Error Correction
    let block_data_len = spec.data_codewords / spec.blocks;
    let mut data_blocks = Vec::with_capacity(spec.blocks);
    let mut ec_blocks = Vec::with_capacity(spec.blocks);

    for b in 0..spec.blocks {
        let block_slice = &data_codewords[b * block_data_len..(b + 1) * block_data_len];
        data_blocks.push(block_slice.to_vec());
        ec_blocks.push(rs_encode(block_slice, spec.ec_codewords_per_block));
    }

    // Interleave data codewords
    let mut final_codewords = Vec::with_capacity(spec.data_codewords + spec.ec_codewords_per_block * spec.blocks);
    for i in 0..block_data_len {
        for b in 0..spec.blocks {
            final_codewords.push(data_blocks[b][i]);
        }
    }

    // Interleave EC codewords
    for i in 0..spec.ec_codewords_per_block {
        for b in 0..spec.blocks {
            final_codewords.push(ec_blocks[b][i]);
        }
    }

    // Convert final codewords to bits
    let mut data_bits = Vec::with_capacity(final_codewords.len() * 8);
    for &cw in &final_codewords {
        for i in (0..8).rev() {
            data_bits.push(((cw >> i) & 1) == 1);
        }
    }

    // 3. Matrix construction
    let size = 17 + 4 * spec.version;
    let mut matrix = vec![vec![false; size]; size];
    let mut is_func = vec![vec![false; size]; size];

    // Finder patterns (top-left, top-right, bottom-left)
    let finder_pos = [(0, 0), (0, size - 7), (size - 7, 0)];
    for &(top_r, top_c) in &finder_pos {
        for dr in 0..7 {
            for dc in 0..7 {
                let r = top_r + dr;
                let c = top_c + dc;
                let is_black = dr == 0 || dr == 6 || dc == 0 || dc == 6 || (dr >= 2 && dr <= 4 && dc >= 2 && dc <= 4);
                matrix[r][c] = is_black;
                is_func[r][c] = true;
            }
        }
    }

    // Separators around finders
    let mark_separator = |r: usize, c: usize, m: &mut [Vec<bool>], f: &mut [Vec<bool>]| {
        if r < size && c < size {
            m[r][c] = false;
            f[r][c] = true;
        }
    };

    // Top-left separators
    for i in 0..8 {
        mark_separator(7, i, &mut matrix, &mut is_func);
        mark_separator(i, 7, &mut matrix, &mut is_func);
    }
    // Top-right separators
    for i in 0..8 {
        mark_separator(7, size - 8 + i, &mut matrix, &mut is_func);
        mark_separator(i, size - 8, &mut matrix, &mut is_func);
    }
    // Bottom-left separators
    for i in 0..8 {
        mark_separator(size - 8, i, &mut matrix, &mut is_func);
        mark_separator(size - 8 + i, 7, &mut matrix, &mut is_func);
    }

    // Timing patterns
    for i in 8..(size - 8) {
        matrix[6][i] = (i % 2) == 0;
        is_func[6][i] = true;
        matrix[i][6] = (i % 2) == 0;
        is_func[i][6] = true;
    }

    // Alignment patterns
    if spec.alignment_coords.len() >= 2 {
        for &r in spec.alignment_coords {
            for &c in spec.alignment_coords {
                // Skip if overlapping finder patterns
                let near_finder = (r < 9 && c < 9) || (r < 9 && c > size - 9) || (r > size - 9 && c < 9);
                if near_finder {
                    continue;
                }
                for dr in -2..=2i32 {
                    for dc in -2..=2i32 {
                        let ar = (r as i32 + dr) as usize;
                        let ac = (c as i32 + dc) as usize;
                        let is_black = dr.abs() == 2 || dc.abs() == 2 || (dr == 0 && dc == 0);
                        matrix[ar][ac] = is_black;
                        is_func[ar][ac] = true;
                    }
                }
            }
        }
    }

    // Dark module
    let dark_row = 4 * spec.version + 9;
    matrix[dark_row][8] = true;
    is_func[dark_row][8] = true;

    // Reserve Format Information areas
    for c in 0..=8 {
        if c != 6 {
            is_func[8][c] = true;
        }
    }
    for r in 0..=8 {
        if r != 6 {
            is_func[r][8] = true;
        }
    }
    for r in (size - 7)..size {
        is_func[r][8] = true;
    }
    for c in (size - 8)..size {
        is_func[8][c] = true;
    }

    // 4. Zig-zag data placement with Mask 0 ((row + col) % 2 == 0)
    let mut bit_idx = 0;
    let mut col = (size - 1) as i32;
    let mut going_up = true;

    while col > 0 {
        if col == 6 {
            col -= 1; // Skip vertical timing column
        }

        let rows: Vec<i32> = if going_up {
            (0..size as i32).rev().collect()
        } else {
            (0..size as i32).collect()
        };

        for r in rows {
            for &c in &[col, col - 1] {
                let ur = r as usize;
                let uc = c as usize;
                if !is_func[ur][uc] {
                    let bit = if bit_idx < data_bits.len() {
                        data_bits[bit_idx]
                    } else {
                        false
                    };
                    bit_idx += 1;

                    // Apply Mask 0: (row + col) % 2 == 0
                    let mask = ((ur + uc) % 2) == 0;
                    matrix[ur][uc] = bit ^ mask;
                }
            }
        }

        col -= 2;
        going_up = !going_up;
    }

    // 5. Write Format Information (Level L, Mask 0 = 0x77c4 = 111011111000100)
    let format_bits: u16 = 0x77c4;
    let get_bit = |i: usize| -> bool { ((format_bits >> (14 - i)) & 1) == 1 };

    let top_left_coords = [
        (8, 0), (8, 1), (8, 2), (8, 3), (8, 4), (8, 5),
        (8, 7), (8, 8), (7, 8), (5, 8), (4, 8), (3, 8),
        (2, 8), (1, 8), (0, 8),
    ];
    for (i, &(r, c)) in top_left_coords.iter().enumerate() {
        matrix[r][c] = get_bit(i);
    }

    let bottom_left_coords = [
        (size - 1, 8), (size - 2, 8), (size - 3, 8), (size - 4, 8),
        (size - 5, 8), (size - 6, 8), (size - 7, 8),
    ];
    for (i, &(r, c)) in bottom_left_coords.iter().enumerate() {
        matrix[r][c] = get_bit(i);
    }

    let top_right_coords = [
        (8, size - 8), (8, size - 7), (8, size - 6), (8, size - 5),
        (8, size - 4), (8, size - 3), (8, size - 2), (8, size - 1),
    ];
    for (i, &(r, c)) in top_right_coords.iter().enumerate() {
        matrix[r][c] = get_bit(7 + i);
    }

    Ok(matrix)
}

/// Renders a QR matrix into an SVG string with crispEdges
pub fn qr_to_svg(matrix: &[Vec<bool>], margin: usize) -> String {
    let size = matrix.len();
    let total_size = size + margin * 2;
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {} {}" shape-rendering="crispEdges">"#,
        total_size, total_size
    );
    svg.push_str(r#"<rect width="100%" height="100%" fill="white"/>"#);
    for (r, row) in matrix.iter().enumerate() {
        for (c, &cell) in row.iter().enumerate() {
            if cell {
                svg.push_str(&format!(
                    r#"<rect x="{}" y="{}" width="1" height="1" fill="black"/>"#,
                    c + margin,
                    r + margin
                ));
            }
        }
    }
    svg.push_str("</svg>");
    svg
}

/// Generates a GDK Texture directly from the SVG string
pub fn qr_to_texture(text: &str, _size_px: i32) -> Result<gdk::Texture, String> {
    let matrix = encode_qr(text)?;
    let svg = qr_to_svg(&matrix, 4);
    let bytes = glib::Bytes::from_owned(svg.into_bytes());
    gdk::Texture::from_bytes(&bytes)
        .map_err(|e| format!("Не удалось создать текстуру QR-кода: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qr_generation_totp_uri() {
        let uri = "otpauth://totp/Contacts:admin?secret=JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP&issuer=Contacts";
        let matrix = encode_qr(uri).expect("QR encoding failed");
        assert!(matrix.len() >= 21);
        assert_eq!(matrix.len(), matrix[0].len());

        // Check finder patterns exist in top-left
        assert!(matrix[0][0]); // corner
        assert!(matrix[3][3]); // center of finder pattern
        assert!(!matrix[1][1]); // inner white border
    }

    #[test]
    fn test_qr_svg_output() {
        let uri = "otpauth://totp/Test:user?secret=JBSWY3DP&issuer=Test";
        let matrix = encode_qr(uri).unwrap();
        let svg = qr_to_svg(&matrix, 4);
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains(r#"fill="black""#));
        assert!(svg.contains(r#"fill="white""#));
    }
}
