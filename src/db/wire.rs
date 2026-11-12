use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

#[derive(Debug)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    #[allow(dead_code)]
    pub affected_rows: u64,
}

pub struct WireClient {
    stream: TcpStream,
}

impl WireClient {
    pub fn connect(host: &str, port: u16, dbname: &str, user: &str, password: &str) -> Result<Self, String> {
        let addr = format!("{}:{}", host, port);
        let mut stream = TcpStream::connect(&addr)
            .map_err(|e| format!("Ошибка TCP подключения к {}: {}", addr, e))?;

        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| e.to_string())?;

        // 1. StartupMessage
        let mut body = Vec::new();
        body.extend_from_slice(&196608i32.to_be_bytes()); // Protocol v3.0
        body.extend_from_slice(b"user\0");
        body.extend_from_slice(user.as_bytes());
        body.push(0);
        body.extend_from_slice(b"database\0");
        body.extend_from_slice(dbname.as_bytes());
        body.push(0);
        body.push(0);

        let total_len = (body.len() + 4) as i32;
        stream
            .write_all(&total_len.to_be_bytes())
            .map_err(|e| format!("Ошибка отправки StartupMessage: {}", e))?;
        stream
            .write_all(&body)
            .map_err(|e| format!("Ошибка отправки данных StartupMessage: {}", e))?;
        stream.flush().map_err(|e| e.to_string())?;

        // 2. Authentication and initialization loop
        loop {
            let mut msg_type = [0u8; 1];
            stream
                .read_exact(&mut msg_type)
                .map_err(|e| format!("Ошибка чтения ответа сервера: {}", e))?;

            let mut len_bytes = [0u8; 4];
            stream
                .read_exact(&mut len_bytes)
                .map_err(|e| format!("Ошибка чтения длины пакета: {}", e))?;
            let len = i32::from_be_bytes(len_bytes) as usize;
            let payload_len = if len >= 4 { len - 4 } else { 0 };

            let mut payload = vec![0u8; payload_len];
            stream
                .read_exact(&mut payload)
                .map_err(|e| format!("Ошибка чтения содержимого пакета: {}", e))?;

            match msg_type[0] {
                b'R' => {
                    // Authentication request
                    if payload.len() < 4 {
                        return Err("Неверный размер пакета авторизации".to_string());
                    }
                    let auth_code = i32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                    match auth_code {
                        0 => {
                            // AuthenticationOk, continue waiting for ReadyForQuery
                        }
                        3 => {
                            // CleartextPassword
                            let mut pass_body = Vec::new();
                            pass_body.extend_from_slice(password.as_bytes());
                            pass_body.push(0);
                            let pass_len = (pass_body.len() + 4) as i32;
                            stream.write_all(b"p").map_err(|e| e.to_string())?;
                            stream.write_all(&pass_len.to_be_bytes()).map_err(|e| e.to_string())?;
                            stream.write_all(&pass_body).map_err(|e| e.to_string())?;
                            stream.flush().map_err(|e| e.to_string())?;
                        }
                        5 => {
                            // MD5Password (salt is next 4 bytes)
                            if payload.len() < 8 {
                                return Err("Недостаточная длина соли MD5".to_string());
                            }
                            let salt = &payload[4..8];
                            let md5_token = md5_auth_token(user, password, salt);
                            let mut pass_body = Vec::new();
                            pass_body.extend_from_slice(md5_token.as_bytes());
                            pass_body.push(0);
                            let pass_len = (pass_body.len() + 4) as i32;
                            stream.write_all(b"p").map_err(|e| e.to_string())?;
                            stream.write_all(&pass_len.to_be_bytes()).map_err(|e| e.to_string())?;
                            stream.write_all(&pass_body).map_err(|e| e.to_string())?;
                            stream.flush().map_err(|e| e.to_string())?;
                        }
                        other => {
                            return Err(format!(
                                "Неподдерживаемый тип авторизации PostgreSQL (код {}). Используйте libpq.",
                                other
                            ));
                        }
                    }
                }
                b'E' => {
                    let err_msg = parse_error_response(&payload);
                    return Err(format!("Ошибка PostgreSQL: {}", err_msg));
                }
                b'Z' => {
                    // ReadyForQuery
                    break;
                }
                _ => {
                    // Discard ParameterStatus ('S'), BackendKeyData ('K'), Notice ('N')
                }
            }
        }

        Ok(Self { stream })
    }

    pub fn execute(&mut self, query: &str) -> Result<QueryResult, String> {
        let mut body = Vec::new();
        body.extend_from_slice(query.as_bytes());
        body.push(0);
        let len = (body.len() + 4) as i32;

        self.stream.write_all(b"Q").map_err(|e| e.to_string())?;
        self.stream.write_all(&len.to_be_bytes()).map_err(|e| e.to_string())?;
        self.stream.write_all(&body).map_err(|e| e.to_string())?;
        self.stream.flush().map_err(|e| e.to_string())?;

        let mut columns = Vec::new();
        let mut rows = Vec::new();
        let mut affected = 0;
        let mut error = None;

        loop {
            let mut msg_type = [0u8; 1];
            self.stream
                .read_exact(&mut msg_type)
                .map_err(|e| format!("Ошибка чтения типа ответа: {}", e))?;

            let mut len_bytes = [0u8; 4];
            self.stream
                .read_exact(&mut len_bytes)
                .map_err(|e| format!("Ошибка чтения длины пакета: {}", e))?;
            let len = i32::from_be_bytes(len_bytes) as usize;
            let payload_len = if len >= 4 { len - 4 } else { 0 };

            let mut payload = vec![0u8; payload_len];
            self.stream
                .read_exact(&mut payload)
                .map_err(|e| format!("Ошибка чтения payload: {}", e))?;

            match msg_type[0] {
                b'T' => {
                    // RowDescription
                    columns = parse_row_description(&payload);
                }
                b'D' => {
                    // DataRow
                    if let Ok(row) = parse_data_row(&payload) {
                        rows.push(row);
                    }
                }
                b'C' => {
                    // CommandComplete
                    let tag = String::from_utf8_lossy(&payload);
                    let words: Vec<&str> = tag.split_whitespace().collect();
                    if let Some(last) = words.last() {
                        affected = last.trim_matches('\0').parse::<u64>().unwrap_or(0);
                    }
                }
                b'E' => {
                    error = Some(parse_error_response(&payload));
                }
                b'Z' => {
                    // ReadyForQuery
                    break;
                }
                _ => {}
            }
        }

        if let Some(err) = error {
            return Err(format!("Ошибка выполнения SQL: {}", err));
        }

        Ok(QueryResult {
            columns,
            rows,
            affected_rows: affected,
        })
    }
}

fn parse_error_response(payload: &[u8]) -> String {
    let mut messages = Vec::new();
    let mut i = 0;
    while i < payload.len() {
        let field_type = payload[i];
        if field_type == 0 {
            break;
        }
        i += 1;
        let mut str_bytes = Vec::new();
        while i < payload.len() && payload[i] != 0 {
            str_bytes.push(payload[i]);
            i += 1;
        }
        i += 1; // skip null
        let s = String::from_utf8_lossy(&str_bytes);
        if field_type == b'M' {
            messages.push(s.to_string());
        }
    }
    if messages.is_empty() {
        String::from_utf8_lossy(payload).to_string()
    } else {
        messages.join("; ")
    }
}

fn parse_row_description(payload: &[u8]) -> Vec<String> {
    if payload.len() < 2 {
        return Vec::new();
    }
    let col_count = i16::from_be_bytes([payload[0], payload[1]]) as usize;
    let mut cols = Vec::with_capacity(col_count);
    let mut idx = 2;

    for _ in 0..col_count {
        if idx >= payload.len() {
            break;
        }
        let mut name_bytes = Vec::new();
        while idx < payload.len() && payload[idx] != 0 {
            name_bytes.push(payload[idx]);
            idx += 1;
        }
        idx += 1; // null
        idx += 18; // skip table_oid(4), col_attr(2), type_oid(4), type_len(2), type_mod(4), format(2)
        cols.push(String::from_utf8_lossy(&name_bytes).to_string());
    }

    cols
}

fn parse_data_row(payload: &[u8]) -> Result<Vec<String>, String> {
    if payload.len() < 2 {
        return Ok(Vec::new());
    }
    let col_count = i16::from_be_bytes([payload[0], payload[1]]) as usize;
    let mut row = Vec::with_capacity(col_count);
    let mut idx = 2;

    for _ in 0..col_count {
        if idx + 4 > payload.len() {
            break;
        }
        let col_len = i32::from_be_bytes([payload[idx], payload[idx + 1], payload[idx + 2], payload[idx + 3]]);
        idx += 4;
        if col_len < 0 {
            row.push(String::new()); // NULL represented as empty string
        } else {
            let ulen = col_len as usize;
            if idx + ulen > payload.len() {
                break;
            }
            let val = String::from_utf8_lossy(&payload[idx..idx + ulen]).to_string();
            row.push(val);
            idx += ulen;
        }
    }

    Ok(row)
}

/// MD5 password hashing for PostgreSQL wire protocol
fn md5_auth_token(user: &str, password: &str, salt: &[u8]) -> String {
    let mut stage1 = Vec::new();
    stage1.extend_from_slice(password.as_bytes());
    stage1.extend_from_slice(user.as_bytes());
    let hash1 = md5_hex(&stage1);

    let mut stage2 = Vec::new();
    stage2.extend_from_slice(hash1.as_bytes());
    stage2.extend_from_slice(salt);
    let hash2 = md5_hex(&stage2);

    format!("md5{}", hash2)
}

/// RFC 1321 MD5 calculation in pure Rust
fn md5_hex(data: &[u8]) -> String {
    let mut a: u32 = 0x67452301;
    let mut b: u32 = 0xefcdab89;
    let mut c: u32 = 0x98badcfe;
    let mut d: u32 = 0x10325476;

    let bit_len = (data.len() as u64) * 8;
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0x00);
    }
    padded.extend_from_slice(&bit_len.to_le_bytes());

    for chunk in padded.chunks(64) {
        let mut m = [0u32; 16];
        for (i, c_bytes) in chunk.chunks(4).enumerate() {
            m[i] = u32::from_le_bytes([c_bytes[0], c_bytes[1], c_bytes[2], c_bytes[3]]);
        }

        let mut aa = a;
        let mut bb = b;
        let mut cc = c;
        let mut dd = d;

        macro_rules! step {
            ($f:expr, $a:ident, $b:ident, $c:ident, $d:ident, $k:expr, $s:expr, $i:expr) => {
                let temp = $a.wrapping_add($f).wrapping_add(m[$k]).wrapping_add($i);
                $a = $b.wrapping_add(temp.rotate_left($s));
            };
        }

        // Round 1
        step!((bb & cc) | (!bb & dd), aa, bb, cc, dd, 0, 7, 0xd76aa478);
        step!((aa & bb) | (!aa & cc), dd, aa, bb, cc, 1, 12, 0xe8c7b756);
        step!((dd & aa) | (!dd & bb), cc, dd, aa, bb, 2, 17, 0x242070db);
        step!((cc & dd) | (!cc & aa), bb, cc, dd, aa, 3, 22, 0xc1bdceee);
        step!((bb & cc) | (!bb & dd), aa, bb, cc, dd, 4, 7, 0xf57c0faf);
        step!((aa & bb) | (!aa & cc), dd, aa, bb, cc, 5, 12, 0x4787c62a);
        step!((dd & aa) | (!dd & bb), cc, dd, aa, bb, 6, 17, 0xa8304613);
        step!((cc & dd) | (!cc & aa), bb, cc, dd, aa, 7, 22, 0xfd469501);
        step!((bb & cc) | (!bb & dd), aa, bb, cc, dd, 8, 7, 0x698098d8);
        step!((aa & bb) | (!aa & cc), dd, aa, bb, cc, 9, 12, 0x8b44f7af);
        step!((dd & aa) | (!dd & bb), cc, dd, aa, bb, 10, 17, 0xffff5bb1);
        step!((cc & dd) | (!cc & aa), bb, cc, dd, aa, 11, 22, 0x895cd7be);
        step!((bb & cc) | (!bb & dd), aa, bb, cc, dd, 12, 7, 0x6b901122);
        step!((aa & bb) | (!aa & cc), dd, aa, bb, cc, 13, 12, 0xfd987193);
        step!((dd & aa) | (!dd & bb), cc, dd, aa, bb, 14, 17, 0xa679438e);
        step!((cc & dd) | (!cc & aa), bb, cc, dd, aa, 15, 22, 0x49b40821);

        // Round 2
        step!((bb & dd) | (cc & !dd), aa, bb, cc, dd, 1, 5, 0xf61e2562);
        step!((aa & cc) | (bb & !cc), dd, aa, bb, cc, 6, 9, 0xc040b340);
        step!((dd & bb) | (aa & !bb), cc, dd, aa, bb, 11, 14, 0x265e5a51);
        step!((cc & aa) | (dd & !aa), bb, cc, dd, aa, 0, 20, 0xe9b6c7aa);
        step!((bb & dd) | (cc & !dd), aa, bb, cc, dd, 5, 5, 0xd62f105d);
        step!((aa & cc) | (bb & !cc), dd, aa, bb, cc, 10, 9, 0x02441453);
        step!((dd & bb) | (aa & !bb), cc, dd, aa, bb, 15, 14, 0xd8a1e681);
        step!((cc & aa) | (dd & !aa), bb, cc, dd, aa, 4, 20, 0xe7d3fbc8);
        step!((bb & dd) | (cc & !dd), aa, bb, cc, dd, 9, 5, 0x21e1cde6);
        step!((aa & cc) | (bb & !cc), dd, aa, bb, cc, 14, 9, 0xc33707d6);
        step!((dd & bb) | (aa & !bb), cc, dd, aa, bb, 3, 14, 0xf4d50d87);
        step!((cc & aa) | (dd & !aa), bb, cc, dd, aa, 8, 20, 0x455a14ed);
        step!((bb & dd) | (cc & !dd), aa, bb, cc, dd, 13, 5, 0xa9e3e905);
        step!((aa & cc) | (bb & !cc), dd, aa, bb, cc, 2, 9, 0xfcefa3f8);
        step!((dd & bb) | (aa & !bb), cc, dd, aa, bb, 7, 14, 0x676f02d9);
        step!((cc & aa) | (dd & !aa), bb, cc, dd, aa, 12, 20, 0x8d2a4c8a);

        // Round 3
        step!(bb ^ cc ^ dd, aa, bb, cc, dd, 5, 4, 0xfffa3942);
        step!(aa ^ bb ^ cc, dd, aa, bb, cc, 8, 11, 0x8771f681);
        step!(dd ^ aa ^ bb, cc, dd, aa, bb, 11, 16, 0x6d9d6122);
        step!(cc ^ dd ^ aa, bb, cc, dd, aa, 14, 23, 0xfde5380c);
        step!(bb ^ cc ^ dd, aa, bb, cc, dd, 1, 4, 0xa4beea44);
        step!(aa ^ bb ^ cc, dd, aa, bb, cc, 4, 11, 0x4bdecfa9);
        step!(dd ^ aa ^ bb, cc, dd, aa, bb, 7, 16, 0xf6bb4b60);
        step!(cc ^ dd ^ aa, bb, cc, dd, aa, 10, 23, 0xbebfbc70);
        step!(bb ^ cc ^ dd, aa, bb, cc, dd, 13, 4, 0x289b7ec6);
        step!(aa ^ bb ^ cc, dd, aa, bb, cc, 0, 11, 0xeaa127fa);
        step!(dd ^ aa ^ bb, cc, dd, aa, bb, 3, 16, 0xd4ef3085);
        step!(cc ^ dd ^ aa, bb, cc, dd, aa, 6, 23, 0x04881d05);
        step!(bb ^ cc ^ dd, aa, bb, cc, dd, 9, 4, 0xd9d4d039);
        step!(aa ^ bb ^ cc, dd, aa, bb, cc, 12, 11, 0xe6db99e5);
        step!(dd ^ aa ^ bb, cc, dd, aa, bb, 15, 16, 0x1fa27cf8);
        step!(cc ^ dd ^ aa, bb, cc, dd, aa, 2, 23, 0xc4ac5665);

        // Round 4
        step!(cc ^ (bb | !dd), aa, bb, cc, dd, 0, 6, 0xf4292244);
        step!(bb ^ (aa | !cc), dd, aa, bb, cc, 7, 10, 0x432aff97);
        step!(aa ^ (dd | !bb), cc, dd, aa, bb, 14, 15, 0xab9423a7);
        step!(dd ^ (cc | !aa), bb, cc, dd, aa, 5, 21, 0xfc93a039);
        step!(cc ^ (bb | !dd), aa, bb, cc, dd, 12, 6, 0x655b59c3);
        step!(bb ^ (aa | !cc), dd, aa, bb, cc, 3, 10, 0x8f0ccc92);
        step!(aa ^ (dd | !bb), cc, dd, aa, bb, 10, 15, 0xffeff47d);
        step!(dd ^ (cc | !aa), bb, cc, dd, aa, 1, 21, 0x85845dd1);
        step!(cc ^ (bb | !dd), aa, bb, cc, dd, 8, 6, 0x6fa87e4f);
        step!(bb ^ (aa | !cc), dd, aa, bb, cc, 15, 10, 0xfe2ce6e0);
        step!(aa ^ (dd | !bb), cc, dd, aa, bb, 6, 15, 0xa3014314);
        step!(dd ^ (cc | !aa), bb, cc, dd, aa, 13, 21, 0x4e0811a1);
        step!(cc ^ (bb | !dd), aa, bb, cc, dd, 4, 6, 0xf7537e82);
        step!(bb ^ (aa | !cc), dd, aa, bb, cc, 11, 10, 0xbd3af235);
        step!(aa ^ (dd | !bb), cc, dd, aa, bb, 2, 15, 0x2ad7d2bb);
        step!(dd ^ (cc | !aa), bb, cc, dd, aa, 9, 21, 0xeb86d391);

        a = a.wrapping_add(aa);
        b = b.wrapping_add(bb);
        c = c.wrapping_add(cc);
        d = d.wrapping_add(dd);
    }

    let mut res = String::with_capacity(32);
    for word in [a, b, c, d] {
        for byte in word.to_le_bytes() {
            res.push_str(&format!("{:02x}", byte));
        }
    }
    res
}
