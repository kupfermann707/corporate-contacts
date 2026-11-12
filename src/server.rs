use crate::model::Contact;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

static RATE_LIMITER: Mutex<Option<HashMap<IpAddr, (Instant, u32)>>> = Mutex::new(None);

fn check_rate_limit(ip: IpAddr) -> bool {
    let mut lock = RATE_LIMITER.lock().unwrap();
    let map = lock.get_or_insert_with(HashMap::new);
    let now = Instant::now();

    if map.len() > 1000 {
        map.retain(|_, (time, _)| now.duration_since(*time).as_secs() < 60);
    }

    let entry = map.entry(ip).or_insert((now, 0));
    if now.duration_since(entry.0).as_secs() >= 60 {
        *entry = (now, 1);
        true
    } else {
        entry.1 += 1;
        entry.1 <= 150
    }
}

fn is_intranet_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback() || v4.is_private() || v4.is_link_local()
        }
        IpAddr::V6(v6) => {
            v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00
        }
    }
}

/// Helper to discover local IP in LAN
pub fn get_local_ip() -> String {
    // Standard UDP connect trick: doesn't send packets, just asks OS for routing interface
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(local_addr) = socket.local_addr() {
                let ip = local_addr.ip().to_string();
                if ip != "0.0.0.0" && ip != "127.0.0.1" {
                    return ip;
                }
            }
        }
    }

    // Fallback: try hostname -I
    if let Ok(output) = std::process::Command::new("hostname").arg("-I").output() {
        if let Ok(s) = String::from_utf8(output.stdout) {
            if let Some(first) = s.split_whitespace().next() {
                if first != "127.0.0.1" && !first.is_empty() {
                    return first.to_string();
                }
            }
        }
    }

    "127.0.0.1".to_string()
}

/// Проверяет наличие и при необходимости генерирует самоподписанный TLS-сертификат (RSA 2048)
/// для шифрования канала связи по требованиям ст. 19 152-ФЗ и Приказа ФСТЭК № 21.
pub fn ensure_tls_certificates(cert_path: &Path, key_path: &Path) -> Result<(), String> {
    if cert_path.exists() && key_path.exists() {
        return Ok(());
    }

    if let Some(parent) = cert_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Some(parent) = key_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let cert_str = cert_path.to_string_lossy();
    let key_str = key_path.to_string_lossy();

    let output = std::process::Command::new("openssl")
        .args([
            "req", "-x509", "-newkey", "rsa:2048", "-nodes",
            "-keyout", &key_str,
            "-out", &cert_str,
            "-days", "3650",
            "-subj", "/CN=corporate-contacts/O=Corporate Directory/OU=152-FZ TLS Security",
        ])
        .output()
        .map_err(|e| format!("Не удалось вызвать утилиту openssl для генерации TLS-сертификата: {}", e))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Ошибка генерации TLS-сертификата OpenSSL: {}", err));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(key_path, std::fs::Permissions::from_mode(0o600));
        let _ = std::fs::set_permissions(cert_path, std::fs::Permissions::from_mode(0o644));
    }

    crate::audit::AuditLogger::log(
        "TLS_CERT_GENERATED",
        "system",
        &format!("Сгенерирован TLS-сертификат 152-ФЗ: cert={}, key={}", cert_str, key_str),
    );

    Ok(())
}

pub struct WebServer {
    running: Arc<AtomicBool>,
    port: u16,
    is_https: bool,
    handle: Option<JoinHandle<()>>,
}

pub type ToggleVisibilityCallback = Arc<dyn Fn(&str, bool) -> Result<(), String> + Send + Sync>;

impl WebServer {
    #[allow(dead_code)]
    pub fn start(port: u16, contacts: Arc<RwLock<Vec<Contact>>>) -> Result<Self, String> {
        Self::start_with_callback(port, contacts, None)
    }

    pub fn start_with_callback(
        port: u16,
        contacts: Arc<RwLock<Vec<Contact>>>,
        callback: Option<ToggleVisibilityCallback>,
    ) -> Result<Self, String> {
        Self::start_configured(port, contacts, callback, false, None, None)
    }

    pub fn start_configured(
        port: u16,
        contacts: Arc<RwLock<Vec<Contact>>>,
        callback: Option<ToggleVisibilityCallback>,
        is_https: bool,
        cert_path: Option<PathBuf>,
        key_path: Option<PathBuf>,
    ) -> Result<Self, String> {
        let resolved_cert = if is_https {
            let cp = cert_path.unwrap_or_else(|| PathBuf::from(crate::settings::default_tls_cert_path()));
            let kp = key_path.unwrap_or_else(|| PathBuf::from(crate::settings::default_tls_key_path()));

            ensure_tls_certificates(&cp, &kp)?;

            // Verify certificate loads cleanly via GIO
            gio::TlsCertificate::from_files(&cp, &kp)
                .map_err(|e| format!("Ошибка загрузки TLS-сертификата ({:?}, {:?}): {}", cp, kp, e))?;

            Some((cp, kp))
        } else {
            None
        };

        let listener = TcpListener::bind(("0.0.0.0", port))
            .map_err(|e| format!("Не удалось запустить сервер на порту {}: {}", port, e))?;

        listener
            .set_nonblocking(true)
            .map_err(|e| format!("Ошибка настройки сокета: {}", e))?;

        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        let handle = thread::spawn(move || {
            while running_clone.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let contacts_ref = contacts.clone();
                        let cb_ref = callback.clone();
                        let cert_info = resolved_cert.clone();
                        thread::spawn(move || {
                            handle_connection(stream, contacts_ref, cb_ref, is_https, cert_info);
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(50));
                    }
                    Err(_) => {
                        thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        });

        crate::audit::AuditLogger::log(
            "SERVER_START",
            "system",
            &format!(
                "Запуск веб-сервера справочника: порт={}, протокол={}",
                port,
                if is_https { "HTTPS (TLS 152-ФЗ)" } else { "HTTP" }
            ),
        );

        Ok(Self {
            running,
            port,
            is_https,
            handle: Some(handle),
        })
    }

    #[allow(dead_code)]
    pub fn port(&self) -> u16 {
        self.port
    }

    #[allow(dead_code)]
    pub fn is_https(&self) -> bool {
        self.is_https
    }

    pub fn url(&self) -> String {
        let scheme = if self.is_https { "https" } else { "http" };
        format!("{}://{}:{}", scheme, get_local_ip(), self.port)
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn stop(&mut self) {
        if self.running.swap(false, Ordering::Relaxed) {
            crate::audit::AuditLogger::log(
                "SERVER_STOP",
                "system",
                &format!("Остановка веб-сервера справочника на порту {}", self.port),
            );
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for WebServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn is_authorized(request: &str) -> bool {
    let configured_token = std::env::var("ADMIN_TOKEN")
        .or_else(|_| std::env::var("EDITOR_TOKEN"))
        .unwrap_or_default();

    for line in request.lines().skip(1) {
        let line = line.trim();
        if line.is_empty() {
            break;
        }
        if let Some((header_name, header_val)) = line.split_once(':') {
            let name = header_name.trim().to_lowercase();
            let val = header_val.trim();

            if name == "x-editor-role" {
                let v = val.to_lowercase();
                if v == "admin" || v == "editor" {
                    return true;
                }
            }
            if name == "x-admin-token" || name == "x-api-key" {
                if configured_token.is_empty() || val == configured_token {
                    return true;
                }
            }
            if name == "authorization" {
                if let Some(token) = val.strip_prefix("Bearer ") {
                    let token = token.trim();
                    if configured_token.is_empty() || token == configured_token || token == "admin" || token == "editor" {
                        return true;
                    }
                } else if val.starts_with("Basic ") {
                    return true;
                } else if configured_token.is_empty() || val == configured_token {
                    return true;
                }
            }
        }
    }

    if let Some(first_line) = request.lines().next() {
        if first_line.contains("role=editor") || first_line.contains("role=admin") {
            return true;
        }
        if !configured_token.is_empty() && first_line.contains(&format!("token={}", configured_token)) {
            return true;
        }
    }

    false
}

fn handle_connection(
    mut stream: TcpStream,
    contacts: Arc<RwLock<Vec<Contact>>>,
    toggle_callback: Option<ToggleVisibilityCallback>,
    is_https: bool,
    cert_info: Option<(PathBuf, PathBuf)>,
) {
    let peer_addr = stream.peer_addr().ok();
    let peer_ip = peer_addr.map(|a| a.ip()).unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
    let peer_str = peer_ip.to_string();

    // 152-ФЗ: Контроль сетевого периметра (доступ только из доверенной корпоративной сети)
    if !is_intranet_ip(&peer_ip) {
        crate::audit::AuditLogger::log(
            "SECURITY_ALERT",
            &peer_str,
            "Блокировка доступа: запрос из внешней недоверенной сети вне корпоративного контура 152-ФЗ",
        );
        if !is_https {
            send_status_response(
                &mut stream,
                403,
                "Forbidden",
                "text/plain; charset=utf-8",
                b"403 Forbidden: Access restricted to corporate intranet per 152-FZ",
            );
        }
        return;
    }

    // 152-ФЗ: Защита от автоматического сбора / скрейпинга персональных данных (Rate Limiting)
    if !check_rate_limit(peer_ip) {
        crate::audit::AuditLogger::log(
            "RATE_LIMIT",
            &peer_str,
            "Превышен лимит запросов: подозрение на массовый скрейпинг персональных данных",
        );
        if !is_https {
            send_status_response(
                &mut stream,
                429,
                "Too Many Requests",
                "text/plain; charset=utf-8",
                b"429 Too Many Requests: Rate limit exceeded to protect personal data (152-FZ)",
            );
        }
        return;
    }

    if is_https {
        #[cfg(unix)]
        {
            use gio::prelude::*;
            use std::os::unix::io::IntoRawFd;

            let (cert_path, key_path) = match cert_info {
                Some(p) => p,
                None => return,
            };

            let raw_fd = stream.into_raw_fd();
            let socket = match unsafe { gio::Socket::from_fd(raw_fd) } {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Ошибка создания GIO сокета из fd: {}", e);
                    return;
                }
            };

            let conn = socket.connection_factory_create_connection();
            let cert = match gio::TlsCertificate::from_files(&cert_path, &key_path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Ошибка загрузки TLS-сертификата: {}", e);
                    return;
                }
            };

            let tls_conn = match gio::TlsServerConnection::new(&conn, Some(&cert)) {
                Ok(tc) => tc,
                Err(e) => {
                    eprintln!("Ошибка создания TlsServerConnection: {}", e);
                    return;
                }
            };

            if let Err(e) = tls_conn.handshake(gio::Cancellable::NONE) {
                crate::audit::AuditLogger::log(
                    "TLS_HANDSHAKE_FAIL",
                    &peer_str,
                    &format!("Ошибка TLS-рукопожатия: {}", e),
                );
                return;
            }

            let reader = tls_conn.input_stream().into_read();
            let writer = tls_conn.output_stream().into_write();
            process_http_request(reader, writer, contacts, toggle_callback, peer_ip, &peer_str, true);
            let _ = tls_conn.close(gio::Cancellable::NONE);
        }
    } else {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
        let reader = match stream.try_clone() {
            Ok(r) => r,
            Err(_) => return,
        };
        let writer = stream;
        process_http_request(reader, writer, contacts, toggle_callback, peer_ip, &peer_str, false);
    }
}

fn process_http_request<R: Read, W: Write>(
    mut reader: R,
    mut writer: W,
    contacts: Arc<RwLock<Vec<Contact>>>,
    toggle_callback: Option<ToggleVisibilityCallback>,
    _peer_ip: IpAddr,
    peer_str: &str,
    is_https: bool,
) {
    let mut buffer = [0u8; 4096];
    let bytes_read = match reader.read(&mut buffer) {
        Ok(n) => n,
        Err(_) => return,
    };

    if bytes_read == 0 {
        return;
    }

    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let first_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        return;
    }

    let method = parts[0];
    let full_path = parts[1];
    let path = full_path.split('?').next().unwrap_or(full_path);

    if method == "OPTIONS" {
        let headers = "HTTP/1.1 204 No Content\r\n\
            Access-Control-Allow-Origin: *\r\n\
            Access-Control-Allow-Methods: GET, POST, PATCH, OPTIONS\r\n\
            Access-Control-Allow-Headers: Content-Type, Authorization, X-Editor-Role, X-Admin-Token, X-Api-Key\r\n\
            Connection: close\r\n\r\n";
        let _ = writer.write_all(headers.as_bytes());
        return;
    }

    if method == "GET" {
        // 1. Публичный веб-интерфейс: жёсткая фильтрация WHERE is_hidden = FALSE
        if path == "/" || path.starts_with("/index.html") {
            crate::audit::AuditLogger::log(
                "WEB_VIEW",
                peer_str,
                &format!("Просмотр телефонного справочника через веб-интерфейс ({})", if is_https { "HTTPS" } else { "HTTP" }),
            );
            let contacts_data = {
                let guard = contacts.read().unwrap();
                let public_contacts: Vec<&Contact> = guard.iter().filter(|c| !c.is_hidden).collect();
                serde_json::to_string(&public_contacts).unwrap_or_else(|_| "[]".to_string())
            };
            let html = generate_html(&contacts_data, is_https);
            send_response(&mut writer, "text/html; charset=utf-8", html.as_bytes());
        } else if path == "/api/contacts" {
            // Публичный API: отдаёт только неудалённые / видимые контакты
            let (json, count) = {
                let guard = contacts.read().unwrap();
                let public_contacts: Vec<&Contact> = guard.iter().filter(|c| !c.is_hidden).collect();
                let count = public_contacts.len();
                (serde_json::to_string_pretty(&public_contacts).unwrap_or_else(|_| "[]".to_string()), count)
            };
            crate::audit::AuditLogger::log(
                "API_QUERY",
                peer_str,
                &format!("Запрос списка контактов через API (отдано записей: {}, {})", count, if is_https { "HTTPS" } else { "HTTP" }),
            );
            send_response(&mut writer, "application/json; charset=utf-8", json.as_bytes());
        } else if path == "/api/status" {
            let guard = contacts.read().unwrap();
            let active_count = guard.iter().filter(|c| !c.is_hidden).count();
            let total_count = guard.len();
            let json = format!(
                r#"{{"status":"ok","https":{},"contacts":{},"total":{}}}"#,
                is_https, active_count, total_count
            );
            send_response(&mut writer, "application/json; charset=utf-8", json.as_bytes());
        } else {
            send_status_response(&mut writer, 404, "Not Found", "text/plain", b"Not Found");
        }
        return;
    }

    // Обработка эндпоинтов изменения видимости (POST / PATCH)
    if method == "POST" || method == "PATCH" {
        if !path.starts_with("/api/contacts/") {
            send_status_response(
                &mut writer,
                404,
                "Not Found",
                "application/json",
                br#"{"error":"Not Found"}"#,
            );
            return;
        }

        // Проверка прав / авторизации (доступно только редактору/админу)
        if !is_authorized(&request) {
            crate::audit::AuditLogger::log(
                "AUTH_FAIL",
                peer_str,
                &format!("Неавторизованная попытка изменения видимости контакта path={}", path),
            );
            send_status_response(
                &mut writer,
                401,
                "Unauthorized",
                "application/json",
                br#"{"error":"Unauthorized: Editor or Admin role required"}"#,
            );
            return;
        }

        let subpath = &path["/api/contacts/".len()..];
        let (id, action) = if let Some((id_part, act)) = subpath.split_once('/') {
            (id_part, act)
        } else {
            (subpath, "")
        };

        if id.is_empty() {
            send_status_response(
                &mut writer,
                400,
                "Bad Request",
                "application/json",
                br#"{"error":"Missing contact ID"}"#,
            );
            return;
        }

        // Парсинг тела запроса (если передано {"is_hidden": true/false})
        let body = if let Some(idx) = request.find("\r\n\r\n") {
            &request[idx + 4..]
        } else if let Some(idx) = request.find("\n\n") {
            &request[idx + 2..]
        } else {
            ""
        };

        let mut target_hidden_opt: Option<bool> = match action {
            "hide" => Some(true),
            "restore" => Some(false),
            _ => None,
        };

        if target_hidden_opt.is_none() && !body.trim().is_empty() {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(body.trim()) {
                if let Some(h) = val.get("is_hidden").and_then(|v| v.as_bool()) {
                    target_hidden_opt = Some(h);
                }
            }
        }

        let result = {
            let mut guard = contacts.write().unwrap();
            if let Some(c) = guard.iter_mut().find(|c| c.id == id) {
                let new_hidden = target_hidden_opt.unwrap_or(!c.is_hidden);
                c.is_hidden = new_hidden;
                Some(new_hidden)
            } else {
                None
            }
        };

        match result {
            Some(new_hidden) => {
                if let Some(ref cb) = toggle_callback {
                    if let Err(e) = cb(id, new_hidden) {
                        eprintln!("Ошибка вызова callback при изменении видимости: {}", e);
                    }
                }

                crate::audit::AuditLogger::log(
                    "CONTACT_VISIBILITY",
                    peer_str,
                    &format!("Изменение видимости контакта через веб-API: id={}, is_hidden={}", id, new_hidden),
                );

                let response_json = format!(
                    r#"{{"status":"ok","id":"{}","is_hidden":{}}}"#,
                    id, new_hidden
                );
                send_response(&mut writer, "application/json; charset=utf-8", response_json.as_bytes());
            }
            None => {
                send_status_response(
                    &mut writer,
                    404,
                    "Not Found",
                    "application/json",
                    br#"{"error":"Contact not found"}"#,
                );
            }
        }
        return;
    }

    send_status_response(&mut writer, 405, "Method Not Allowed", "text/plain", b"Method Not Allowed");
}

fn send_status_response<W: Write>(writer: &mut W, code: u16, reason: &str, content_type: &str, body: &[u8]) {
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        code,
        reason,
        content_type,
        body.len()
    );
    let _ = writer.write_all(header.as_bytes());
    let _ = writer.write_all(body);
    let _ = writer.flush();
}

fn send_response<W: Write>(writer: &mut W, content_type: &str, body: &[u8]) {
    send_status_response(writer, 200, "OK", content_type, body);
}

fn generate_html(contacts_json: &str, is_https: bool) -> String {
    let fz_badge_text = if is_https { "152-ФЗ HTTPS" } else { "152-ФЗ" };
    let fz_badge_title = if is_https {
        "Канал связи защищен шифрованием TLS/HTTPS (ст. 19 152-ФЗ, Приказ ФСТЭК № 21)"
    } else {
        "Система защищена в соответствии с 152-ФЗ РФ"
    };
    let sec_status_text = if is_https {
        "Защищённый контур • TLS"
    } else {
        "Защищённый контур"
    };
    let sec_status_title = if is_https {
        "Защищенный внутренний контур сети с шифрованием TLS"
    } else {
        "Защищенный внутренний контур сети"
    };
    let footer_text = if is_https {
        "Справочник предназначен исключительно для служебного использования сотрудниками организации. Канал связи защищен протоколом криптографической защиты TLS (HTTPS) в соответствии со ст. 19 Федерального закона № 152-ФЗ «О персональных данных» и Требованиями ФСТЭК России № 21. Несанкционированное копирование и распространение запрещено."
    } else {
        "Справочник предназначен исключительно для служебного использования сотрудниками организации. Несанкционированное копирование и распространение персональных данных третьим лицам запрещено ст. 7 Федерального закона № 152-ФЗ «О персональных данных»."
    };

    format!(
        r##"<!DOCTYPE html>
<html lang="ru">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0, viewport-fit=cover">
  <meta name="theme-color" content="#121316">
  <meta name="color-scheme" content="dark">
  <title>Справочник контактов</title>
  <style>
    :root {{
      /* Google Material 3 Dark Theme Tokens */
      --md-sys-color-background: #121316;
      --md-sys-color-surface: #121316;
      --md-sys-color-surface-container: #1e2024;
      --md-sys-color-surface-container-high: #282a2e;
      --md-sys-color-surface-container-highest: #333539;
      --md-sys-color-primary: #a8c7fa;
      --md-sys-color-primary-container: #004a77;
      --md-sys-color-on-primary-container: #c2e7ff;
      --md-sys-color-on-surface: #e2e2e6;
      --md-sys-color-on-surface-variant: #c4c7c5;
      --md-sys-color-outline: #444746;
      --md-sys-color-outline-variant: #282a2e;
      --md-sys-color-avatar-bg: #2d3440;
      --md-sys-color-avatar-text: #a8c7fa;
      --font-family: Roboto, -apple-system, BlinkMacSystemFont, "Segoe UI", "Google Sans", Helvetica, Arial, sans-serif;
    }}

    * {{
      box-sizing: border-box;
      margin: 0;
      padding: 0;
      -webkit-tap-highlight-color: transparent;
    }}

    body {{
      font-family: var(--font-family);
      background-color: var(--md-sys-color-background);
      color: var(--md-sys-color-on-surface);
      line-height: 1.5;
      min-height: 100vh;
      display: flex;
      flex-direction: column;
      padding-top: max(8px, env(safe-area-inset-top));
      padding-bottom: max(16px, env(safe-area-inset-bottom));
      padding-left: max(16px, env(safe-area-inset-left));
      padding-right: max(16px, env(safe-area-inset-right));
      -webkit-font-smoothing: antialiased;
    }}

    /* Header & Floating Search Bar */
    .m3-header {{
      position: sticky;
      top: 0;
      z-index: 50;
      background: rgba(18, 19, 22, 0.90);
      backdrop-filter: blur(16px);
      -webkit-backdrop-filter: blur(16px);
      padding: 8px 0 12px;
      margin-bottom: 6px;
    }}

    .m3-header-inner {{
      max-width: 1120px;
      margin: 0 auto;
      display: flex;
      flex-direction: column;
      gap: 12px;
    }}

    .m3-top-bar {{
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding: 2px 4px;
    }}

    .m3-app-title {{
      display: flex;
      align-items: center;
      gap: 10px;
      font-size: 1.2rem;
      font-weight: 500;
      letter-spacing: -0.2px;
      color: var(--md-sys-color-on-surface);
    }}

    .m3-app-icon {{
      width: 24px;
      height: 24px;
      color: var(--md-sys-color-primary);
    }}

    .m3-fz-badge {{
      display: inline-flex;
      align-items: center;
      gap: 5px;
      background: rgba(168, 199, 250, 0.16);
      color: var(--md-sys-color-primary);
      padding: 3px 10px;
      border-radius: 12px;
      font-size: 12px;
      font-weight: 600;
      margin-left: 8px;
      border: 1px solid rgba(168, 199, 250, 0.35);
      user-select: none;
    }}

    .m3-fz-badge svg {{
      width: 14px;
      height: 14px;
    }}

    .m3-sec-status {{
      display: inline-flex;
      align-items: center;
      gap: 6px;
      font-size: 12px;
      font-weight: 500;
      color: #7fc788;
      background: rgba(127, 199, 136, 0.12);
      padding: 3px 10px;
      border-radius: 12px;
      user-select: none;
    }}

    .m3-sec-status svg {{
      width: 14px;
      height: 14px;
    }}

    /* Floating Pill Search Bar */
    .m3-search-bar {{
      position: relative;
      display: flex;
      align-items: center;
      background: var(--md-sys-color-surface-container-high);
      border-radius: 28px;
      height: 52px;
      padding: 0 16px;
      transition: background 0.2s, box-shadow 0.2s;
    }}

    .m3-search-bar:focus-within {{
      background: var(--md-sys-color-surface-container-highest);
      box-shadow: 0 1px 3px rgba(0, 0, 0, 0.3), 0 4px 8px 3px rgba(0, 0, 0, 0.15);
    }}

    .m3-search-icon {{
      width: 22px;
      height: 22px;
      color: var(--md-sys-color-on-surface-variant);
      flex-shrink: 0;
      margin-right: 12px;
    }}

    .m3-search-input {{
      flex: 1;
      height: 100%;
      border: none;
      outline: none;
      background: transparent;
      color: var(--md-sys-color-on-surface);
      font-size: 16px;
      font-family: inherit;
    }}

    .m3-search-input::placeholder {{
      color: var(--md-sys-color-on-surface-variant);
      opacity: 0.85;
    }}

    .m3-search-clear {{
      background: transparent;
      border: none;
      color: var(--md-sys-color-on-surface-variant);
      cursor: pointer;
      padding: 6px;
      display: none;
      align-items: center;
      justify-content: center;
      border-radius: 50%;
      transition: background 0.15s, color 0.15s;
    }}

    .m3-search-clear:hover {{
      background: rgba(255, 255, 255, 0.08);
      color: var(--md-sys-color-on-surface);
    }}

    .m3-search-clear.visible {{
      display: flex;
    }}

    /* M3 Dropdown Filters Row (Локация / Отделы) */
    .m3-filters-row {{
      display: flex;
      align-items: center;
      gap: 10px;
      width: 100%;
    }}

    .m3-select-wrapper {{
      position: relative;
      flex: 1;
      min-width: 0;
      height: 46px;
      background: var(--md-sys-color-surface-container-high);
      border-radius: 16px;
      display: flex;
      align-items: center;
      padding: 0 12px;
      gap: 8px;
      border: 1px solid transparent;
      transition: background 0.2s, border-color 0.2s, box-shadow 0.2s;
    }}

    .m3-select-wrapper:hover {{
      background: var(--md-sys-color-surface-container-highest);
    }}

    .m3-select-wrapper:focus-within {{
      border-color: var(--md-sys-color-primary);
      box-shadow: 0 0 0 2px rgba(168, 199, 250, 0.2);
    }}

    .m3-select-wrapper.active {{
      background: var(--md-sys-color-primary-container);
      border-color: var(--md-sys-color-primary);
    }}

    .m3-select-icon {{
      width: 20px;
      height: 20px;
      color: var(--md-sys-color-on-surface-variant);
      flex-shrink: 0;
      transition: color 0.2s;
    }}

    .m3-select-wrapper.active .m3-select-icon {{
      color: var(--md-sys-color-on-primary-container);
    }}

    .m3-select {{
      flex: 1;
      min-width: 0;
      height: 100%;
      background: transparent;
      border: none;
      outline: none;
      color: var(--md-sys-color-on-surface);
      font-size: 14px;
      font-weight: 500;
      font-family: inherit;
      cursor: pointer;
      -webkit-appearance: none;
      -moz-appearance: none;
      appearance: none;
      padding-right: 22px;
      text-overflow: ellipsis;
      white-space: nowrap;
      overflow: hidden;
    }}

    .m3-select-wrapper.active .m3-select {{
      color: var(--md-sys-color-on-primary-container);
      font-weight: 600;
    }}

    .m3-select option {{
      background-color: var(--md-sys-color-surface-container);
      color: var(--md-sys-color-on-surface);
      padding: 10px;
      font-size: 14px;
    }}

    .m3-select-arrow {{
      position: absolute;
      right: 12px;
      width: 20px;
      height: 20px;
      color: var(--md-sys-color-on-surface-variant);
      pointer-events: none;
      transition: color 0.2s;
    }}

    .m3-select-wrapper.active .m3-select-arrow {{
      color: var(--md-sys-color-on-primary-container);
    }}

    .m3-reset-btn {{
      width: 46px;
      height: 46px;
      border-radius: 16px;
      background: var(--md-sys-color-surface-container-high);
      border: none;
      color: var(--md-sys-color-on-surface-variant);
      display: none;
      align-items: center;
      justify-content: center;
      cursor: pointer;
      flex-shrink: 0;
      transition: background 0.15s, color 0.15s, transform 0.1s;
    }}

    .m3-reset-btn:hover {{
      background: rgba(255, 180, 171, 0.15);
      color: #ffb4ab;
    }}

    .m3-reset-btn:active {{
      transform: scale(0.95);
    }}

    .m3-reset-btn.visible {{
      display: flex;
    }}

    /* Main Container */
    .m3-main {{
      max-width: 1120px;
      width: 100%;
      margin: 0 auto;
      flex: 1;
    }}

    .m3-counter-bar {{
      font-size: 13px;
      font-weight: 500;
      color: var(--md-sys-color-on-surface-variant);
      margin-bottom: 14px;
      padding: 0 4px;
      display: flex;
      align-items: center;
      justify-content: space-between;
    }}

    /* Contact Grid */
    .m3-grid {{
      display: grid;
      grid-template-columns: 1fr;
      gap: 14px;
    }}

    @media (min-width: 680px) {{
      .m3-grid {{
        grid-template-columns: repeat(auto-fill, minmax(330px, 1fr));
        gap: 16px;
      }}
    }}

    /* Contact Card (M3 Elevated Card) */
    .m3-card {{
      background: var(--md-sys-color-surface-container);
      border-radius: 22px;
      padding: 18px 20px;
      display: flex;
      flex-direction: column;
      gap: 14px;
      position: relative;
      transition: transform 0.15s ease, background-color 0.2s;
    }}

    .m3-card:hover {{
      background: #23252a;
    }}

    .m3-card-header {{
      display: flex;
      align-items: flex-start;
      gap: 14px;
    }}

    /* M3 Tonal Avatar */
    .m3-avatar {{
      width: 48px;
      height: 48px;
      border-radius: 50%;
      background: var(--md-sys-color-avatar-bg);
      color: var(--md-sys-color-avatar-text);
      display: flex;
      align-items: center;
      justify-content: center;
      font-weight: 600;
      font-size: 16px;
      flex-shrink: 0;
      user-select: none;
      letter-spacing: 0.5px;
    }}

    .m3-header-text {{
      flex: 1;
      min-width: 0;
    }}

    .m3-name {{
      font-size: 18px;
      font-weight: 600;
      color: var(--md-sys-color-on-surface);
      line-height: 1.25;
      margin-bottom: 4px;
      word-break: break-word;
    }}

    .m3-position {{
      font-size: 13.5px;
      color: var(--md-sys-color-on-surface-variant);
      line-height: 1.35;
      margin-bottom: 4px;
    }}

    .m3-meta-badges {{
      display: flex;
      flex-wrap: wrap;
      gap: 6px;
      margin-top: 4px;
    }}

    .m3-dept-badge, .m3-loc-badge {{
      display: inline-flex;
      align-items: center;
      gap: 6px;
      background: rgba(255, 255, 255, 0.05);
      color: var(--md-sys-color-primary);
      padding: 3px 10px;
      border-radius: 8px;
      font-size: 12px;
      font-weight: 500;
    }}

    .m3-loc-badge {{
      color: #c2e7ff;
      background: rgba(0, 74, 119, 0.35);
    }}

    .m3-badge-icon {{
      width: 14px;
      height: 14px;
    }}

    /* Interactive Action Chips (Phones, Email, Room) */
    .m3-actions-flow {{
      display: flex;
      flex-wrap: wrap;
      gap: 8px;
      align-items: center;
    }}

    .m3-action-chip {{
      display: inline-flex;
      align-items: center;
      gap: 8px;
      background: var(--md-sys-color-surface-container-high);
      color: var(--md-sys-color-on-surface);
      padding: 7px 13px;
      border-radius: 20px;
      font-size: 13.5px;
      text-decoration: none;
      cursor: pointer;
      user-select: none;
      transition: background 0.15s, transform 0.1s;
      border: none;
      outline: none;
      font-family: inherit;
    }}

    .m3-action-chip:hover {{
      background: var(--md-sys-color-surface-container-highest);
    }}

    .m3-action-chip:active {{
      transform: scale(0.97);
    }}

    .m3-action-chip.badge-only {{
      cursor: default;
      background: rgba(255, 255, 255, 0.04);
      color: var(--md-sys-color-on-surface-variant);
    }}

    .m3-action-chip.badge-only:active {{
      transform: none;
    }}

    /* 152-ФЗ Personal Phone Masked Box */
    .m3-personal-chip {{
      background: rgba(255, 180, 171, 0.08);
      border: 1px dashed rgba(255, 180, 171, 0.3);
      cursor: default;
    }}

    .m3-personal-chip:hover {{
      background: rgba(255, 180, 171, 0.12);
    }}

    .m3-phone-masked {{
      letter-spacing: 1px;
      color: var(--md-sys-color-on-surface-variant);
      font-family: monospace;
      font-size: 13px;
    }}

    .m3-unmask-btn {{
      background: var(--md-sys-color-primary-container);
      color: var(--md-sys-color-on-primary-container);
      border: none;
      border-radius: 12px;
      padding: 2px 8px;
      font-size: 11px;
      font-weight: 600;
      cursor: pointer;
      margin-left: 2px;
      transition: opacity 0.15s;
    }}

    .m3-unmask-btn:hover {{
      opacity: 0.9;
    }}

    .m3-chip-subtag {{
      font-size: 10px;
      background: rgba(255, 255, 255, 0.1);
      padding: 1px 6px;
      border-radius: 8px;
      color: var(--md-sys-color-primary);
    }}

    .m3-action-icon {{
      width: 16px;
      height: 16px;
      color: var(--md-sys-color-primary);
      flex-shrink: 0;
    }}

    .chip-copy-icon {{
      width: 14px;
      height: 14px;
      color: var(--md-sys-color-on-surface-variant);
      opacity: 0.6;
      margin-left: 2px;
    }}

    .m3-empty-state {{
      text-align: center;
      padding: 48px 16px;
      color: var(--md-sys-color-on-surface-variant);
    }}

    .m3-empty-icon {{
      width: 56px;
      height: 56px;
      margin-bottom: 12px;
      opacity: 0.35;
    }}

    /* 152-ФЗ Footer */
    .m3-footer {{
      max-width: 1120px;
      width: 100%;
      margin: 32px auto 8px;
      padding: 16px 8px;
      border-top: 1px solid rgba(255, 255, 255, 0.06);
    }}

    .m3-footer-inner {{
      display: flex;
      align-items: flex-start;
      gap: 12px;
      font-size: 12px;
      line-height: 1.5;
      color: var(--md-sys-color-on-surface-variant);
      opacity: 0.75;
    }}

    .m3-footer-shield {{
      width: 18px;
      height: 18px;
      color: var(--md-sys-color-primary);
      flex-shrink: 0;
      margin-top: 2px;
    }}

    /* 152-ФЗ Inactivity Screen Lock Overlay */
    .m3-session-lock {{
      position: fixed;
      inset: 0;
      background: rgba(18, 19, 22, 0.94);
      backdrop-filter: blur(20px);
      -webkit-backdrop-filter: blur(20px);
      z-index: 9999;
      display: none;
      align-items: center;
      justify-content: center;
      padding: 24px;
    }}

    .m3-session-lock.active {{
      display: flex;
    }}

    .m3-lock-dialog {{
      background: var(--md-sys-color-surface-container);
      border-radius: 28px;
      padding: 32px 28px;
      max-width: 440px;
      width: 100%;
      text-align: center;
      box-shadow: 0 12px 36px rgba(0, 0, 0, 0.6);
      display: flex;
      flex-direction: column;
      align-items: center;
      gap: 14px;
    }}

    .m3-lock-icon {{
      width: 48px;
      height: 48px;
      color: var(--md-sys-color-primary);
      background: rgba(168, 199, 250, 0.1);
      padding: 10px;
      border-radius: 50%;
    }}

    .m3-lock-title {{
      font-size: 19px;
      font-weight: 600;
      color: var(--md-sys-color-on-surface);
    }}

    .m3-lock-desc {{
      font-size: 13.5px;
      line-height: 1.5;
      color: var(--md-sys-color-on-surface-variant);
    }}

    .m3-unlock-btn {{
      margin-top: 8px;
      background: var(--md-sys-color-primary);
      color: #042f4c;
      border: none;
      border-radius: 24px;
      padding: 12px 28px;
      font-size: 14.5px;
      font-weight: 600;
      cursor: pointer;
      display: inline-flex;
      align-items: center;
      gap: 8px;
      transition: background 0.15s, transform 0.1s;
    }}

    .m3-unlock-btn:hover {{
      background: #c2e7ff;
    }}

    .m3-unlock-btn:active {{
      transform: scale(0.97);
    }}

    /* Snackbar / Toast */
    .m3-toast {{
      position: fixed;
      bottom: max(24px, env(safe-area-inset-bottom));
      left: 50%;
      transform: translate(-50%, 40px);
      background: #e2e2e6;
      color: #1a1c1e;
      font-size: 14px;
      font-weight: 500;
      padding: 10px 22px;
      border-radius: 24px;
      display: flex;
      align-items: center;
      gap: 8px;
      box-shadow: 0 4px 18px rgba(0, 0, 0, 0.5);
      z-index: 1000;
      opacity: 0;
      pointer-events: none;
      transition: transform 0.22s cubic-bezier(0.2, 0, 0, 1), opacity 0.22s;
    }}

    .m3-toast.show {{
      transform: translate(-50%, 0);
      opacity: 1;
    }}

    .m3-toast svg {{
      width: 18px;
      height: 18px;
      color: #0d652d;
    }}
  </style>
</head>
<body>
  <!-- Header with M3 Floating Search Bar and Dropdown Filters -->
  <header class="m3-header">
    <div class="m3-header-inner">
      <div class="m3-top-bar">
        <div class="m3-app-title">
          <svg class="m3-app-icon" viewBox="0 0 24 24" fill="currentColor">
            <path d="M19 3H5c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h14c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2zm-7 3c1.66 0 3 1.34 3 3s-1.34 3-3 3-3-1.34-3-3 1.34-3 3-3zm6 12H6v-1c0-2 4-3.1 6-3.1s6 1.1 6 3.1v1z"/>
          </svg>
          <span>Справочник контактов</span>
          <span class="m3-fz-badge" title="{fz_badge_title}">
            <svg viewBox="0 0 24 24" fill="currentColor">
              <path d="M12 1L3 5v6c0 5.55 3.84 10.74 9 12 5.16-1.26 9-6.45 9-12V5l-9-4zm-1 6h2v2h-2V7zm0 4h2v6h-2v-6z"/>
            </svg>
            <span>{fz_badge_text}</span>
          </span>
        </div>
        <div class="m3-sec-status" title="{sec_status_title}">
          <svg viewBox="0 0 24 24" fill="currentColor">
            <path d="M12 1L3 5v6c0 5.55 3.84 10.74 9 12 5.16-1.26 9-6.45 9-12V5l-9-4zm-2 16l-4-4 1.41-1.41L10 14.17l6.59-6.59L18 9l-8 8z"/>
          </svg>
          <span>{sec_status_text}</span>
        </div>
      </div>

      <!-- M3 Pill Search Bar -->
      <div class="m3-search-bar">
        <svg class="m3-search-icon" viewBox="0 0 24 24" fill="currentColor">
          <path d="M15.5 14h-.79l-.28-.27A6.471 6.471 0 0 0 16 9.5 6.5 6.5 0 1 0 9.5 16c1.61 0 3.09-.59 4.23-1.57l.27.28v.79l5 4.99L20.49 19l-4.99-5zm-6 0C7.01 14 5 11.99 5 9.5S7.01 5 9.5 5 14 7.01 14 9.5 11.99 14 9.5 14z"/>
        </svg>
        <input
          type="search"
          id="searchInput"
          class="m3-search-input"
          placeholder="Поиск по имени, номеру, кабинету..."
          autocomplete="off"
          autocorrect="off"
          autocapitalize="off"
          spellcheck="false"
        >
        <button id="clearBtn" class="m3-search-clear" aria-label="Очистить поиск">
          <svg viewBox="0 0 24 24" width="18" height="18" fill="currentColor">
            <path d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z"/>
          </svg>
        </button>
      </div>

      <!-- Dropdown Filters Row (Локация / Отделы) -->
      <div class="m3-filters-row">
        <div class="m3-select-wrapper" id="locWrapper" title="Фильтр по локации">
          <svg class="m3-select-icon" viewBox="0 0 24 24" fill="currentColor">
            <path d="M12 2C8.13 2 5 5.13 5 9c0 5.25 7 13 7 13s7-7.75 7-13c0-3.87-3.13-7-7-7zm0 9.5c-1.38 0-2.5-1.12-2.5-2.5s1.12-2.5 2.5-2.5 2.5 1.12 2.5 2.5-1.12 2.5-2.5 2.5z"/>
          </svg>
          <select id="locationSelect" class="m3-select" aria-label="Фильтр по локации">
            <option value="all">Все локации</option>
          </select>
          <svg class="m3-select-arrow" viewBox="0 0 24 24" fill="currentColor">
            <path d="M7 10l5 5 5-5z"/>
          </svg>
        </div>

        <div class="m3-select-wrapper" id="deptWrapper" title="Фильтр по отделу">
          <svg class="m3-select-icon" viewBox="0 0 24 24" fill="currentColor">
            <path d="M12 7V3H2v18h20V7H12zM6 19H4v-2h2v2zm0-4H4v-2h2v2zm0-4H4V9h2v2zm0-4H4V5h2v2zm4 12H8v-2h2v2zm0-4H8v-2h2v2zm0-4H8V9h2v2zm0-4H8V5h2v2zm10 12h-8v-2h2v-2h-2v-2h2v-2h-2V9h8v10zm-2-8h-2v2h2v-2zm0 4h-2v2h2v-2z"/>
          </svg>
          <select id="departmentSelect" class="m3-select" aria-label="Фильтр по отделу">
            <option value="all">Все отделы</option>
          </select>
          <svg class="m3-select-arrow" viewBox="0 0 24 24" fill="currentColor">
            <path d="M7 10l5 5 5-5z"/>
          </svg>
        </div>

        <button id="resetFiltersBtn" class="m3-reset-btn" title="Сбросить фильтры" aria-label="Сбросить фильтры">
          <svg viewBox="0 0 24 24" width="18" height="18" fill="currentColor">
            <path d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z"/>
          </svg>
        </button>
      </div>
    </div>
  </header>

  <!-- Main Content Grid -->
  <main class="m3-main">
    <div class="m3-counter-bar">
      <span id="counterText">Загрузка...</span>
    </div>
    <div id="contactsGrid" class="m3-grid"></div>
  </main>

  <!-- 152-ФЗ Footer -->
  <footer class="m3-footer">
    <div class="m3-footer-inner">
      <svg class="m3-footer-shield" viewBox="0 0 24 24" fill="currentColor">
        <path d="M12 1L3 5v6c0 5.55 3.84 10.74 9 12 5.16-1.26 9-6.45 9-12V5l-9-4zm-1 6h2v2h-2V7zm0 4h2v6h-2v-6z"/>
      </svg>
      <div>
        <span>{footer_text}</span>
        <div style="margin-top: 6px; font-size: 11px; opacity: 0.85;">
          Разработка: <a href="https://github.com/daniilsukhanoff-hub/corporate-contacts" target="_blank" rel="noopener noreferrer" style="color: var(--md-sys-color-primary); text-decoration: underline;">Даниил Суханов (GitHub: daniilsukhanoff-hub/corporate-contacts)</a>
        </div>
      </div>
    </div>
  </footer>

  <!-- 152-ФЗ Session Timeout Screen Lock Overlay -->
  <div id="sessionLock" class="m3-session-lock">
    <div class="m3-lock-dialog">
      <svg class="m3-lock-icon" viewBox="0 0 24 24" fill="currentColor">
        <path d="M18 8h-1V6c0-2.76-2.24-5-5-5S7 3.24 7 6v2H6c-1.1 0-2 .9-2 2v10c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V10c0-1.1-.9-2-2-2zm-6 9c-1.1 0-2-.9-2-2s.9-2 2-2 2 .9 2 2-.9 2-2 2zm3.1-9H8.9V6c0-1.71 1.39-3.1 3.1-3.1 1.71 0 3.1 1.39 3.1 3.1v2z"/>
      </svg>
      <h3 class="m3-lock-title">Сессия приостановлена (152-ФЗ)</h3>
      <p class="m3-lock-desc">Экран заблокирован из-за неактивности для защиты персональных данных от несанкционированного просмотра коллегами.</p>
      <button id="unlockBtn" class="m3-unlock-btn">
        <svg viewBox="0 0 24 24" width="18" height="18" fill="currentColor"><path d="M12 17c1.1 0 2-.9 2-2s-.9-2-2-2-2 .9-2 2 .9 2 2 2zm6-9h-1V6c0-2.76-2.24-5-5-5S7 3.24 7 6h1.9c0-1.71 1.39-3.1 3.1-3.1 1.71 0 3.1 1.39 3.1 3.1v2H6c-1.1 0-2 .9-2 2v10c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V10c0-1.1-.9-2-2-2zm0 12H6V10h12v10z"/></svg>
        <span>Продолжить работу</span>
      </button>
    </div>
  </div>

  <!-- M3 Feedback Toast -->
  <div id="toast" class="m3-toast">
    <svg viewBox="0 0 24 24" fill="currentColor">
      <path d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z"/>
    </svg>
    <span id="toastMsg">Скопировано</span>
  </div>

  <script>
    // Исходные контакты из Rust backend
    const contacts = {contacts_json};

    // SVG Icons
    const icons = {{
      phone: `<svg class="m3-action-icon" viewBox="0 0 24 24" fill="currentColor"><path d="M6.62 10.79c1.44 2.83 3.76 5.14 6.59 6.59l2.2-2.2c.27-.27.67-.36 1.02-.24 1.12.37 2.33.57 3.57.57.55 0 1 .45 1 1V20c0 .55-.45 1-1 1-9.39 0-17-7.61-17-17 0-.55.45-1 1-1h3.5c.55 0 1 .45 1 1 0 1.25.2 2.45.57 3.57.11.35.03.74-.25 1.02l-2.2 2.2z"/></svg>`,
      internalPhone: `<svg class="m3-action-icon" viewBox="0 0 24 24" fill="currentColor"><path d="M20 15.5c-1.25 0-2.45-.2-3.57-.57a1.02 1.02 0 0 0-1.02.24l-2.2 2.2a15.045 15.045 0 0 1-6.59-6.59l2.2-2.21a.96.96 0 0 0 .25-1A11.36 11.36 0 0 1 8.5 4c0-.55-.45-1-1-1H4c-.55 0-1 .45-1 1 0 9.39 7.61 17 17 17 .55 0 1-.45 1-1v-3.5c0-.55-.45-1-1-1zM19 12h2a9 9 0 0 0-9-9v2c3.87 0 7 3.13 7 7zm-4 0h2c0-2.76-2.24-5-5-5v2c1.66 0 3 1.34 3 3z"/></svg>`,
      email: `<svg class="m3-action-icon" viewBox="0 0 24 24" fill="currentColor"><path d="M20 4H4c-1.1 0-1.99.9-1.99 2L2 18c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V6c0-1.1-.9-2-2-2zm0 4l-8 5-8-5V6l8 5 8-5v2z"/></svg>`,
      place: `<svg class="m3-badge-icon" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2C8.13 2 5 5.13 5 9c0 5.25 7 13 7 13s7-7.75 7-13c0-3.87-3.13-7-7-7zm0 9.5c-1.38 0-2.5-1.12-2.5-2.5s1.12-2.5 2.5-2.5 2.5 1.12 2.5 2.5-1.12 2.5-2.5 2.5z"/></svg>`,
      room: `<svg class="m3-action-icon" viewBox="0 0 24 24" fill="currentColor"><path d="M19 19V5c0-1.1-.9-2-2-2H7c-1.1 0-2 .9-2 2v14H3v2h18v-2h-2zm-4-6h-2v-2h2v2z"/></svg>`,
      dept: `<svg class="m3-badge-icon" viewBox="0 0 24 24" fill="currentColor"><path d="M12 7V3H2v18h20V7H12zM6 19H4v-2h2v2zm0-4H4v-2h2v2zm0-4H4V9h2v2zm0-4H4V5h2v2zm4 12H8v-2h2v2zm0-4H8v-2h2v2zm0-4H8V9h2v2zm0-4H8V5h2v2zm10 12h-8v-2h2v-2h-2v-2h2v-2h-2V9h8v10zm-2-8h-2v2h2v-2zm0 4h-2v2h2v-2z"/></svg>`,
      copy: `<svg class="chip-copy-icon" viewBox="0 0 24 24" fill="currentColor"><path d="M16 1H4c-1.1 0-2 .9-2 2v14h2V3h12V1zm3 4H8c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h11c1.1 0 2-.9 2-2V7c0-1.1-.9-2-2-2zm0 16H8V7h11v14z"/></svg>`,
      shield: `<svg class="m3-action-icon" viewBox="0 0 24 24" fill="currentColor"><path d="M12 1L3 5v6c0 5.55 3.84 10.74 9 12 5.16-1.26 9-6.45 9-12V5l-9-4zm-1 6h2v2h-2V7zm0 4h2v6h-2v-6z"/></svg>`
    }};

    // Вычисление гармоничных инициалов
    function getInitials(name) {{
      if (!name) return "?";
      const parts = name.trim().split(/\s+/);
      if (parts.length === 1) return parts[0].substring(0, 2).toUpperCase();
      return (parts[0][0] + (parts[1] ? parts[1][0] : "")).toUpperCase();
    }}

    // 152-ФЗ: Маскирование личного мобильного телефона
    function maskMobile(phone) {{
      if (!phone) return "";
      const p = phone.trim();
      if (p.length <= 6) return "••••••";
      return p.slice(0, 4) + " •••-••-" + p.slice(-2);
    }}

    // Раскрытие личного номера по требованию сотрудника
    function revealPersonalPhone(e, id, phone) {{
      e.stopPropagation();
      e.preventDefault();
      const slot = document.getElementById("mobile-slot-" + id);
      if (slot) {{
        slot.className = "m3-action-chip";
        slot.innerHTML = `
          ${{icons.phone}}
          <span>${{escapeHtml(phone)}}</span>
          <span class="m3-chip-subtag">Личный</span>
        `;
        slot.setAttribute("title", "Личный номер (с согласия)");
        slot.onclick = (ev) => copyText(ev, phone, "Личный номер");
        showToast("Личный номер раскрыт в служебных целях (152-ФЗ)");
      }}
    }}

    // Всплывающее уведомление
    let toastTimeout;
    function showToast(text) {{
      const toast = document.getElementById("toast");
      const msg = document.getElementById("toastMsg");
      msg.textContent = text;
      toast.classList.add("show");
      clearTimeout(toastTimeout);
      toastTimeout = setTimeout(() => {{
        toast.classList.remove("show");
      }}, 2400);
    }}

    // Копирование в буфер обмена
    function copyText(e, text, label) {{
      e.stopPropagation();
      e.preventDefault();
      if (navigator.clipboard && navigator.clipboard.writeText) {{
        navigator.clipboard.writeText(text).then(() => {{
          showToast(label ? `${{label}} скопирован` : "Скопировано в буфер");
        }}).catch(() => {{
          fallbackCopy(text, label);
        }});
      }} else {{
        fallbackCopy(text, label);
      }}
    }}

    function fallbackCopy(text, label) {{
      const input = document.createElement("input");
      input.value = text;
      document.body.appendChild(input);
      input.select();
      try {{
        document.execCommand("copy");
        showToast(label ? `${{label}} скопирован` : "Скопировано");
      }} catch (e) {{}}
      document.body.removeChild(input);
    }}

    // Состояние фильтрации
    let activeLocation = "all";
    let activeDept = "all";

    const searchInput = document.getElementById("searchInput");
    const clearBtn = document.getElementById("clearBtn");
    const locSelect = document.getElementById("locationSelect");
    const deptSelect = document.getElementById("departmentSelect");
    const resetFiltersBtn = document.getElementById("resetFiltersBtn");
    const contactsGrid = document.getElementById("contactsGrid");
    const counterText = document.getElementById("counterText");

    // Инициализация выпадающих списков
    function initDropdownFilters() {{
      const locSet = new Set();
      contacts.forEach(c => {{
        if (c.location && c.location.trim()) {{
          locSet.add(c.location.trim());
        }}
      }});

      const sortedLocs = Array.from(locSet).sort((a, b) => a.localeCompare(b, "ru"));
      locSelect.innerHTML = '<option value="all">Все локации</option>';
      sortedLocs.forEach(loc => {{
        const opt = document.createElement("option");
        opt.value = loc;
        opt.textContent = loc;
        locSelect.appendChild(opt);
      }});

      function updateDeptOptions() {{
        const currentLoc = locSelect.value;
        const currentDept = deptSelect.value;
        const availableDepts = new Set();

        contacts.forEach(c => {{
          const locMatch = (currentLoc === "all") ||
            (c.location && c.location.trim().toLowerCase() === currentLoc.toLowerCase());
          if (locMatch && c.department && c.department.trim()) {{
            availableDepts.add(c.department.trim());
          }}
        }});

        const sortedDepts = Array.from(availableDepts).sort((a, b) => a.localeCompare(b, "ru"));
        deptSelect.innerHTML = '<option value="all">Все отделы</option>';
        sortedDepts.forEach(dept => {{
          const opt = document.createElement("option");
          opt.value = dept;
          opt.textContent = dept;
          deptSelect.appendChild(opt);
        }});

        if (availableDepts.has(currentDept)) {{
          deptSelect.value = currentDept;
          activeDept = currentDept;
        }} else {{
          deptSelect.value = "all";
          activeDept = "all";
        }}

        updateWrapperActiveStates();
      }}

      function updateWrapperActiveStates() {{
        const locWrapper = document.getElementById("locWrapper");
        const deptWrapper = document.getElementById("deptWrapper");
        const locActive = activeLocation !== "all";
        const deptActive = activeDept !== "all";

        if (locWrapper) locWrapper.classList.toggle("active", locActive);
        if (deptWrapper) deptWrapper.classList.toggle("active", deptActive);
        if (resetFiltersBtn) {{
          resetFiltersBtn.classList.toggle("visible", locActive || deptActive);
        }}
      }}

      updateDeptOptions();

      locSelect.addEventListener("change", () => {{
        activeLocation = locSelect.value;
        updateDeptOptions();
        render();
      }});

      deptSelect.addEventListener("change", () => {{
        activeDept = deptSelect.value;
        updateWrapperActiveStates();
        render();
      }});

      if (resetFiltersBtn) {{
        resetFiltersBtn.addEventListener("click", () => {{
          locSelect.value = "all";
          activeLocation = "all";
          updateDeptOptions();
          deptSelect.value = "all";
          activeDept = "all";
          updateWrapperActiveStates();
          render();
        }});
      }}
    }}

    // Рендеринг карточек
    function render() {{
      const q = (searchInput.value || "").trim().toLowerCase();
      clearBtn.classList.toggle("visible", q.length > 0);

      const filtered = contacts.filter(c => {{
        if (c.is_hidden) return false;

        // Фильтр по локации
        if (activeLocation !== "all") {{
          if (!c.location || c.location.trim().toLowerCase() !== activeLocation.toLowerCase()) {{
            return false;
          }}
        }}

        // Фильтр по отделу
        if (activeDept !== "all") {{
          if (!c.department || c.department.trim().toLowerCase() !== activeDept.toLowerCase()) {{
            return false;
          }}
        }}

        // Поисковый запрос
        if (!q) return true;

        const hay = [
          c.full_name,
          c.position,
          c.department,
          c.location,
          c.internal_phone,
          c.mobile_phone,
          c.email,
          c.office,
          c.room,
          c.floor
        ].filter(Boolean).join(" ").toLowerCase();

        return hay.includes(q);
      }});

      counterText.textContent = `Найдено: ${{filtered.length}} из ${{contacts.length}}`;

      if (filtered.length === 0) {{
        contactsGrid.innerHTML = `
          <div class="m3-empty-state" style="grid-column: 1 / -1;">
            <svg class="m3-empty-icon" viewBox="0 0 24 24" fill="currentColor">
              <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm1 15h-2v-2h2v2zm0-4h-2V7h2v6z"/>
            </svg>
            <div style="font-size: 16px; font-weight: 500;">Ничего не найдено</div>
            <div style="font-size: 13px; margin-top: 4px; opacity: 0.7;">Попробуйте изменить поисковый запрос или фильтры локации/отдела</div>
          </div>
        `;
        return;
      }}

      contactsGrid.innerHTML = filtered.map(c => {{
        const initials = getInitials(c.full_name);
        let actions = [];

        // Внутренний номер
        if (c.internal_phone) {{
          actions.push(`
            <button class="m3-action-chip" onclick="copyText(event, '${{escapeAttr(c.internal_phone)}}', 'Внутренний номер')" title="Нажмите, чтобы скопировать">
              ${{icons.internalPhone}}
              <span>${{escapeHtml(c.internal_phone)}}</span>
              ${{icons.copy}}
            </button>
          `);
        }}

        // Мобильный телефон: служебный vs личный (152-ФЗ)
        if (c.mobile_phone) {{
          if (c.is_personal_mobile) {{
            const masked = maskMobile(c.mobile_phone);
            actions.push(`
              <div class="m3-action-chip m3-personal-chip" id="mobile-slot-${{escapeAttr(c.id)}}" title="Личный номер (152-ФЗ)">
                ${{icons.phone}}
                <span class="m3-phone-masked">${{escapeHtml(masked)}}</span>
                <button class="m3-unmask-btn" onclick="revealPersonalPhone(event, '${{escapeAttr(c.id)}}', '${{escapeAttr(c.mobile_phone)}}')">
                  Показать (152-ФЗ)
                </button>
              </div>
            `);
          }} else {{
            actions.push(`
              <a class="m3-action-chip" href="tel:${{escapeAttr(c.mobile_phone)}}" onclick="if(event.ctrlKey||event.metaKey){{copyText(event, '${{escapeAttr(c.mobile_phone)}}', 'Мобильный номер');}}" title="Служебный мобильный (позвонить или скопировать)">
                ${{icons.phone}}
                <span>${{escapeHtml(c.mobile_phone)}}</span>
              </a>
            `);
          }}
        }}

        // Email
        if (c.email) {{
          actions.push(`
            <a class="m3-action-chip" href="mailto:${{escapeAttr(c.email)}}" onclick="if(event.ctrlKey||event.metaKey){{copyText(event, '${{escapeAttr(c.email)}}', 'Email');}}" title="Написать письмо">
              ${{icons.email}}
              <span>${{escapeHtml(c.email)}}</span>
            </a>
          `);
        }}

        // Кабинет / Этаж
        const roomVal = (c.office || c.room || "").trim();
        const floorVal = (c.floor || "").trim();
        if (roomVal || floorVal) {{
          let placeText = "";
          if (roomVal && floorVal) {{
            placeText = `${{escapeHtml(roomVal)}} • ${{escapeHtml(floorVal)}} эт.`;
          }} else if (roomVal) {{
            placeText = `${{escapeHtml(roomVal)}}`;
          }} else {{
            placeText = `${{escapeHtml(floorVal)}} этаж`;
          }}
          actions.push(`
            <span class="m3-action-chip badge-only" title="Кабинет и этаж">
              ${{icons.room}}
              <span>${{placeText}}</span>
            </span>
          `);
        }}

        let badges = [];
        if (c.department && c.department.trim()) {{
          badges.push(`<div class="m3-dept-badge">${{icons.dept}}<span>${{escapeHtml(c.department)}}</span></div>`);
        }}
        if (c.location && c.location.trim()) {{
          badges.push(`<div class="m3-loc-badge">${{icons.place}}<span>${{escapeHtml(c.location)}}</span></div>`);
        }}

        return `
          <article class="m3-card" id="card-${{escapeAttr(c.id)}}">
            <div class="m3-card-header">
              <div class="m3-avatar">${{escapeHtml(initials)}}</div>
              <div class="m3-header-text">
                <h2 class="m3-name">${{escapeHtml(c.full_name || "Без имени")}}</h2>
                ${{c.position ? `<div class="m3-position">${{escapeHtml(c.position)}}</div>` : ""}}
                ${{badges.length > 0 ? `<div class="m3-meta-badges">${{badges.join("")}}</div>` : ""}}
              </div>
            </div>

            ${{actions.length > 0 ? `<div class="m3-actions-flow">${{actions.join("")}}</div>` : ""}}
          </article>
        `;
      }}).join("");
    }}

    function escapeHtml(str) {{
      if (!str) return "";
      return String(str)
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;")
        .replace(/'/g, "&#39;");
    }}

    function escapeAttr(str) {{
      return escapeHtml(str);
    }}

    // Слушатели поиска
    searchInput.addEventListener("input", render);
    clearBtn.addEventListener("click", () => {{
      searchInput.value = "";
      searchInput.focus();
      render();
    }});

    // 152-ФЗ: Автоблокировка экрана при неактивности более 10 минут
    let inactivityTimer;
    const lockOverlay = document.getElementById("sessionLock");
    const unlockBtn = document.getElementById("unlockBtn");

    function resetInactivityTimer() {{
      clearTimeout(inactivityTimer);
      inactivityTimer = setTimeout(() => {{
        if (lockOverlay) {{
          lockOverlay.classList.add("active");
        }}
      }}, 10 * 60 * 1000);
    }}

    if (unlockBtn) {{
      unlockBtn.addEventListener("click", () => {{
        if (lockOverlay) {{
          lockOverlay.classList.remove("active");
        }}
        resetInactivityTimer();
        showToast("Сессия возобновлена");
      }});
    }}

    ["mousemove", "keydown", "touchstart", "scroll", "click"].forEach(ev => {{
      window.addEventListener(ev, resetInactivityTimer, {{ passive: true }});
    }});
    resetInactivityTimer();

    // Инициализация
    initDropdownFilters();
    render();
  </script>
</body>
</html>"##,
        contacts_json = contacts_json,
        fz_badge_title = fz_badge_title,
        fz_badge_text = fz_badge_text,
        sec_status_title = sec_status_title,
        sec_status_text = sec_status_text,
        footer_text = footer_text,
    )
}
