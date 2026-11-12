#[cfg(test)]
mod tests {
    use corporate_contacts::model::Contact;
    use corporate_contacts::server::WebServer;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;

    #[test]
    fn test_web_server_endpoints() {
        let contacts = vec![
            Contact::new(
                "Центральный офис".to_string(),
                "ИТ".to_string(),
                "Разработчик".to_string(),
                "Тестовый Пользователь".to_string(),
                "1234".to_string(),
                "89991234567".to_string(),
                "test@company.local".to_string(),
                "3".to_string(),
                "301".to_string(),
            )
        ];
        let shared = Arc::new(RwLock::new(contacts));

        let port = 18080;
        let mut server = WebServer::start(port, shared).expect("Failed to start server");
        assert!(server.is_running());

        // Test GET /
        {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("Failed to connect");
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
            let mut res = String::new();
            stream.read_to_string(&mut res).unwrap();
            assert!(res.contains("HTTP/1.1 200 OK"));
            assert!(res.contains("Справочник контактов"));
            assert!(res.contains("Тестовый Пользователь"));
        }

        // Test GET /api/contacts
        {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("Failed to connect");
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            stream.write_all(b"GET /api/contacts HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
            let mut res = String::new();
            stream.read_to_string(&mut res).unwrap();
            assert!(res.contains("HTTP/1.1 200 OK"));
            assert!(res.contains("application/json"));
            assert!(res.contains("Тестовый Пользователь"));
        }

        // Test GET /api/status
        {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("Failed to connect");
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            stream.write_all(b"GET /api/status HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
            let mut res = String::new();
            stream.read_to_string(&mut res).unwrap();
            assert!(res.contains("HTTP/1.1 200 OK"));
            assert!(res.contains(r#""status":"ok""#));
        }

        server.stop();
        assert!(!server.is_running());
    }

    #[test]
    fn test_soft_delete_and_visibility_endpoints() {
        let mut active_contact = Contact::new(
            "Офис 1".to_string(),
            "ИТ".to_string(),
            "Разработчик".to_string(),
            "Активный Контакт".to_string(),
            "1111".to_string(),
            "+7 900 111-22-33".to_string(),
            "active@corp.org".to_string(),
            "2".to_string(),
            "201".to_string(),
        );
        active_contact.id = "contact-active-1".to_string();
        active_contact.is_hidden = false;

        let mut hidden_contact = Contact::new(
            "Офис 2".to_string(),
            "Бухгалтерия".to_string(),
            "Бухгалтер".to_string(),
            "Скрытый Контакт".to_string(),
            "2222".to_string(),
            "+7 900 222-33-44".to_string(),
            "hidden@corp.org".to_string(),
            "3".to_string(),
            "305".to_string(),
        );
        hidden_contact.id = "contact-hidden-2".to_string();
        hidden_contact.is_hidden = true;

        let shared = Arc::new(RwLock::new(vec![active_contact, hidden_contact]));

        let callback_invocations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let cb_invocations_clone = callback_invocations.clone();
        let toggle_cb: corporate_contacts::server::ToggleVisibilityCallback = Arc::new(move |_id, _hidden| {
            cb_invocations_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        });

        let port = 18081;
        let mut server = WebServer::start_with_callback(port, shared.clone(), Some(toggle_cb))
            .expect("Failed to start server with callback");

        // 1. Verify GET / and /api/contacts strictly hide is_hidden = true
        {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("Failed to connect");
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
            let mut res = String::new();
            stream.read_to_string(&mut res).unwrap();
            assert!(res.contains("Активный Контакт"));
            assert!(!res.contains("Скрытый Контакт"), "Hidden contact must not be returned in public UI");
        }

        {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("Failed to connect");
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            stream.write_all(b"GET /api/contacts HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
            let mut res = String::new();
            stream.read_to_string(&mut res).unwrap();
            assert!(res.contains("Активный Контакт"));
            assert!(!res.contains("Скрытый Контакт"), "Hidden contact must not be returned in public API");
        }

        // 2. Unauthorized request to toggle visibility should return 401
        {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("Failed to connect");
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            stream.write_all(b"POST /api/contacts/contact-active-1/toggle-visibility HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            let mut res = String::new();
            stream.read_to_string(&mut res).unwrap();
            assert!(res.contains("401 Unauthorized"), "Must require authorization: {}", res);
        }

        // 3. Authorized request (X-Editor-Role: editor) should succeed and toggle visibility
        {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("Failed to connect");
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            stream.write_all(b"POST /api/contacts/contact-active-1/toggle-visibility HTTP/1.1\r\nHost: localhost\r\nX-Editor-Role: editor\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            let mut res = String::new();
            stream.read_to_string(&mut res).unwrap();
            assert!(res.contains("200 OK"), "Authorized toggle should return 200 OK: {}", res);
            assert!(res.contains(r#""is_hidden":true"#));
            assert_eq!(callback_invocations.load(std::sync::atomic::Ordering::SeqCst), 1);
        }

        // Now active_contact should also be hidden from public API
        {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("Failed to connect");
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            stream.write_all(b"GET /api/contacts HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
            let mut res = String::new();
            stream.read_to_string(&mut res).unwrap();
            assert!(!res.contains("Активный Контакт"), "Contact that was soft-deleted must not appear in public API");
        }

        // 4. Restore endpoint with Authorization header
        {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("Failed to connect");
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            stream.write_all(b"POST /api/contacts/contact-active-1/restore HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            let mut res = String::new();
            stream.read_to_string(&mut res).unwrap();
            assert!(res.contains("200 OK"), "Restore should return 200 OK: {}", res);
            assert!(res.contains(r#""is_hidden":false"#));
            assert_eq!(callback_invocations.load(std::sync::atomic::Ordering::SeqCst), 2);
        }

        // Now active_contact should be visible again in public API
        {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("Failed to connect");
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            stream.write_all(b"GET /api/contacts HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
            let mut res = String::new();
            stream.read_to_string(&mut res).unwrap();
            assert!(res.contains("Активный Контакт"), "Restored contact must appear in public API");
        }

        server.stop();
    }

    #[test]
    fn test_tls_server_connection() {
        use gio::prelude::*;

        // Generate cert if not exists
        let cert_dir = std::env::temp_dir().join("test_contacts_tls");
        let _ = std::fs::create_dir_all(&cert_dir);
        let cert_path = cert_dir.join("cert.pem");
        let key_path = cert_dir.join("key.pem");

        if !cert_path.exists() || !key_path.exists() {
            let status = std::process::Command::new("openssl")
                .args([
                    "req", "-x509", "-newkey", "rsa:2048", "-nodes",
                    "-keyout", key_path.to_str().unwrap(),
                    "-out", cert_path.to_str().unwrap(),
                    "-days", "1",
                    "-subj", "/CN=localhost",
                ])
                .status()
                .expect("Failed to execute openssl");
            assert!(status.success());
        }

        let cert_path_clone = cert_path.clone();
        let key_path_clone = key_path.clone();
        let port = 18443;
        let listener = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();

        let handle = std::thread::spawn(move || {
            use std::os::unix::io::IntoRawFd;
            let (stream, _) = listener.accept().unwrap();
            let raw_fd = stream.into_raw_fd();

            let cert = gio::TlsCertificate::from_files(&cert_path_clone, &key_path_clone)
                .expect("Failed to load certificate files");

            let socket = unsafe { gio::Socket::from_fd(raw_fd) }.expect("Failed to create Socket from fd");
            let conn = socket.connection_factory_create_connection();

            let tls_conn = gio::TlsServerConnection::new(&conn, Some(&cert))
                .expect("Failed to create TlsServerConnection");
            tls_conn.handshake(gio::Cancellable::NONE).expect("Handshake failed");
            let mut reader = tls_conn.input_stream().into_read();
            let mut writer = tls_conn.output_stream().into_write();

            let mut buf = [0u8; 1024];
            let n = reader.read(&mut buf).expect("Read failed");
            let req = String::from_utf8_lossy(&buf[..n]);
            assert!(req.contains("GET /test"));

            writer.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nHELLO").expect("Write failed");
            let _ = writer.flush();
            let _ = tls_conn.close(gio::Cancellable::NONE);
        });

        // Run curl -k to make request
        std::thread::sleep(Duration::from_millis(100));
        let output = std::process::Command::new("curl")
            .args(["-k", "-s", "-i", &format!("https://127.0.0.1:{}/test", port)])
            .output()
            .expect("Failed to run curl");

        let resp = String::from_utf8_lossy(&output.stdout);
        assert!(resp.contains("HTTP/1.1 200 OK"), "Resp was: {}", resp);
        assert!(resp.contains("HELLO"));

        handle.join().unwrap();
    }

    #[test]
    fn test_web_server_https_full_roundtrip() {
        let contacts = vec![
            Contact::new(
                "Главный офис".to_string(),
                "Информационная безопасность".to_string(),
                "Специалист 152-ФЗ".to_string(),
                "Иванов Иван".to_string(),
                "7777".to_string(),
                "+79991234567".to_string(),
                "ivanov@company.local".to_string(),
                "5".to_string(),
                "505".to_string(),
            )
        ];
        let shared = Arc::new(RwLock::new(contacts));

        let port = 18444;
        let cert_dir = std::env::temp_dir().join("test_contacts_full_https");
        let cert_path = cert_dir.join("cert.pem");
        let key_path = cert_dir.join("key.pem");

        let mut server = WebServer::start_configured(
            port,
            shared,
            None,
            true,
            Some(cert_path),
            Some(key_path),
        ).expect("Failed to start HTTPS server");

        assert!(server.is_running());
        assert!(server.is_https());
        assert!(server.url().starts_with("https://"));

        // Wait briefly for server accept loop
        std::thread::sleep(Duration::from_millis(150));

        // 1. Test GET / via HTTPS
        let output = std::process::Command::new("curl")
            .args(["-k", "-s", "-i", &format!("https://127.0.0.1:{}/", port)])
            .output()
            .expect("Failed to execute curl");
        let resp = String::from_utf8_lossy(&output.stdout);
        assert!(resp.contains("HTTP/1.1 200 OK"), "Expected 200 OK, got: {}", resp);
        assert!(resp.contains("152-ФЗ HTTPS"), "HTML must contain 152-ФЗ HTTPS badge");
        assert!(resp.contains("Защищённый контур • TLS"), "HTML must contain TLS perimeter indicator");
        assert!(resp.contains("Канал связи защищен протоколом криптографической защиты TLS (HTTPS)"), "Footer must contain TLS 152-FZ notice");
        assert!(resp.contains("Иванов Иван"));

        // 2. Test GET /api/status via HTTPS
        let output = std::process::Command::new("curl")
            .args(["-k", "-s", &format!("https://127.0.0.1:{}/api/status", port)])
            .output()
            .expect("Failed to execute curl");
        let json_resp = String::from_utf8_lossy(&output.stdout);
        assert!(json_resp.contains(r#""status":"ok""#));
        assert!(json_resp.contains(r#""https":true"#));

        // 3. Test GET /api/contacts via HTTPS
        let output = std::process::Command::new("curl")
            .args(["-k", "-s", &format!("https://127.0.0.1:{}/api/contacts", port)])
            .output()
            .expect("Failed to execute curl");
        let contacts_resp = String::from_utf8_lossy(&output.stdout);
        assert!(contacts_resp.contains("Иванов Иван"));
        assert!(contacts_resp.contains("7777"));

        server.stop();
        assert!(!server.is_running());
    }
}
