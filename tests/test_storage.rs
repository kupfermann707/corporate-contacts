#[cfg(test)]
mod tests {
    use corporate_contacts::storage::StorageManager;
    use corporate_contacts::model::Contact;

    #[test]
    fn test_empty_load_contacts() {
        let storage = StorageManager::new();
        let contacts = storage.load_contacts().expect("Failed to load contacts");
        // Fresh database is empty
        assert!(contacts.is_empty(), "Contacts list should initially be empty in clean state");
    }

    #[test]
    fn test_contact_matching_and_initials() {
        let c = Contact::new(
            "Филиал Восток".to_string(),
            "ИТ Департамент".to_string(),
            "Ведущий разработчик".to_string(),
            "Иванов Иван Иванович".to_string(),
            "123".to_string(),
            "+7 999 123-45-67".to_string(),
            "ivanov@company.local".to_string(),
            "4 этаж".to_string(),
            "402".to_string(),
        );

        assert_eq!(c.initials(), "ИИ");
        assert!(c.matches("иванов", "", ""));
        assert!(c.matches("разработчик", "", ""));
        assert!(c.matches("123", "", ""));
        assert!(c.matches("", "Филиал Восток", ""));
        assert!(c.matches("", "", "ИТ Департамент"));
        assert!(!c.matches("", "Другой Филиал", ""));
        assert!(!c.matches("", "", "Бухгалтерия"));
    }

    #[test]
    fn test_csv_import_and_merge() {
        let csv_content = "\u{feff}Локация;Отдел;Должность;ФИО;Внутренний телефон;Мобильный телефон;Email;Этаж / Корпус;Кабинет\r\n\
\"Центральный офис\";\"IT отдел\";\"Ведущий инженер\";\"Тестов Тест Тестович\";\"1234\";\"+7 999 111-22-33\";\"test@company.local\";\"3 этаж\";\"301\"\r\n\
\"Филиал Север\";\"Бухгалтерия\";\"Главный бухгалтер\";\"Иванова Анна\";\"5678\";\"+7 999 444-55-66\";\"ivanova@company.local\";\"2\";\"204\"\r\n";

        let (contacts, report) = StorageManager::parse_csv_content(csv_content).expect("Failed to parse CSV");
        assert_eq!(contacts.len(), 2);
        assert_eq!(report.recognized_columns.len(), 9);
        assert_eq!(report.delimiter, ';');

        let c1 = &contacts[0];
        assert_eq!(c1.full_name, "Тестов Тест Тестович");
        assert_eq!(c1.location, "Центральный офис");
        assert_eq!(c1.department, "IT отдел");
        assert_eq!(c1.position, "Ведущий инженер");
        assert_eq!(c1.internal_phone, "1234");
        assert_eq!(c1.mobile_phone, "+7 999 111-22-33");
        assert_eq!(c1.email, "test@company.local");
        assert_eq!(c1.floor, "3 этаж");
        assert_eq!(c1.office, "301");
        assert_eq!(c1.room(), "301");

        let c2 = &contacts[1];
        assert_eq!(c2.full_name, "Иванова Анна");
        assert_eq!(c2.location, "Филиал Север");

        // Test comma delimited with English headers
        let comma_csv = "Full Name,Department,Position,Phone,Email\n\
\"Smith John\",\"Sales\",\"Manager\",\"+1 555 1234\",\"john@example.com\"\n";
        let (contacts2, report2) = StorageManager::parse_csv_content(comma_csv).expect("Failed to parse comma CSV");
        assert_eq!(contacts2.len(), 1);
        assert_eq!(report2.delimiter, ',');
        assert_eq!(contacts2[0].full_name, "Smith John");
        assert_eq!(contacts2[0].department, "Sales");
        assert_eq!(contacts2[0].position, "Manager");

        // Test merging
        let mut base_list = contacts.clone();
        let update_csv = "ФИО;Отдел;Внутренний телефон;Кабинет\n\
\"Тестов Тест Тестович\";\"IT отдел\";\"9999\";\"305\"\n\
\"Новиков Петр\";\"Склад\";\"7777\";\"101\"\n";
        let (update_contacts, _) = StorageManager::parse_csv_content(update_csv).expect("Failed to parse update CSV");
        assert_eq!(update_contacts.len(), 2);

        let merged_count = StorageManager::merge_contacts(&mut base_list, update_contacts);
        assert_eq!(merged_count, 2);
        assert_eq!(base_list.len(), 3); // 1 updated + 1 added + 1 existing

        let updated_testov = base_list.iter().find(|c| c.full_name == "Тестов Тест Тестович").unwrap();
        assert_eq!(updated_testov.internal_phone, "9999");
        assert_eq!(updated_testov.office, "305");
        assert_eq!(updated_testov.room(), "305");
        // Old fields preserved if empty in update
        assert_eq!(updated_testov.mobile_phone, "+7 999 111-22-33");
        assert_eq!(updated_testov.email, "test@company.local");
    }

    #[test]
    fn test_database_settings_configuration() {
        use corporate_contacts::settings::AppSettings;

        let mut settings = AppSettings::default();
        // Initially empty
        assert_eq!(settings.db_host, "");
        assert_eq!(settings.db_port, "");
        assert!(!settings.is_db_configured(), "Default settings should not be configured");

        // Only host is filled
        settings.db_host = "192.168.1.50".to_string();
        assert!(!settings.is_db_configured(), "Should be false when port is empty");

        // Whitespace only port
        settings.db_port = "   ".to_string();
        assert!(!settings.is_db_configured(), "Should be false when port is whitespace");

        // Both host and port filled
        settings.db_port = "5432".to_string();
        assert!(settings.is_db_configured(), "Should be true when both host and port are filled");

        // Whitespace host
        settings.db_host = "   ".to_string();
        assert!(!settings.is_db_configured(), "Should be false when host is whitespace");
    }

    #[test]
    fn test_settings_save_and_load_roundtrip() {
        use corporate_contacts::settings::AppSettings;
        let temp_dir = std::env::temp_dir().join("test_settings_roundtrip");
        let _ = std::fs::create_dir_all(&temp_dir);
        let path = temp_dir.join("settings.json");

        let mut settings = AppSettings::default();
        settings.db_host = "db.company.internal".to_string();
        settings.db_port = "5432".to_string();
        settings.db_password = "SensitiveDbPassword#2026".to_string();

        assert!(settings.save_to_path(&path).is_ok());

        // Ensure file is encrypted on disk
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\"encrypted\": true"));
        assert!(!raw.contains("SensitiveDbPassword#2026"));

        let loaded = AppSettings::load_from_path(&path);
        assert_eq!(loaded.db_host, "db.company.internal");
        assert_eq!(loaded.db_password, "SensitiveDbPassword#2026");
        assert!(loaded.https_enabled, "HTTPS must be enabled by default per 152-FZ");
        assert_eq!(loaded.web_server_port, 8443, "Default HTTPS port should be 8443");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_totp_settings_persistence() {
        use corporate_contacts::settings::AppSettings;
        use corporate_contacts::totp::{generate_totp_secret, verify_totp, compute_totp_code, decode_base32};

        let temp_dir = std::env::temp_dir().join("test_totp_persistence");
        let _ = std::fs::create_dir_all(&temp_dir);
        let path = temp_dir.join("settings.json");

        let mut settings = AppSettings::default();
        let secret = generate_totp_secret();
        settings.totp_enabled = true;
        settings.totp_secret = secret.clone();

        assert!(settings.save_to_path(&path).is_ok());

        // File is encrypted on disk and doesn't reveal the secret
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\"encrypted\": true"));
        assert!(!raw.contains(&secret));

        let loaded = AppSettings::load_from_path(&path);
        assert!(loaded.totp_enabled);
        assert_eq!(loaded.totp_secret, secret);

        // Verify that TOTP code verification works with loaded secret
        let secret_bytes = decode_base32(&loaded.totp_secret).unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let code = compute_totp_code(&secret_bytes, now, 6);
        assert!(verify_totp(&loaded.totp_secret, &code));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_field_mapper_bidirectional() {
        use corporate_contacts::field_mapping::{ContactField, FieldMapper};

        // 1. English SQL columns ↔ ContactField
        assert_eq!(ContactField::from_name("id"), Some(ContactField::Id));
        assert_eq!(ContactField::from_name("location"), Some(ContactField::Location));
        assert_eq!(ContactField::from_name("department"), Some(ContactField::Department));
        assert_eq!(ContactField::from_name("position"), Some(ContactField::Position));
        assert_eq!(ContactField::from_name("full_name"), Some(ContactField::FullName));
        assert_eq!(ContactField::from_name("internal_phone"), Some(ContactField::InternalPhone));
        assert_eq!(ContactField::from_name("mobile_phone"), Some(ContactField::MobilePhone));
        assert_eq!(ContactField::from_name("email"), Some(ContactField::Email));
        assert_eq!(ContactField::from_name("floor"), Some(ContactField::Floor));
        assert_eq!(ContactField::from_name("office"), Some(ContactField::Office));
        assert_eq!(ContactField::from_name("room"), Some(ContactField::Office));

        // 2. Russian UI/CSV headers ↔ ContactField
        assert_eq!(ContactField::from_name("Локация"), Some(ContactField::Location));
        assert_eq!(ContactField::from_name("Локация / Филиал"), Some(ContactField::Location));
        assert_eq!(ContactField::from_name("Филиал"), Some(ContactField::Location));
        assert_eq!(ContactField::from_name("Отдел"), Some(ContactField::Department));
        assert_eq!(ContactField::from_name("Должность"), Some(ContactField::Position));
        assert_eq!(ContactField::from_name("ФИО"), Some(ContactField::FullName));
        assert_eq!(ContactField::from_name("Внутренний телефон"), Some(ContactField::InternalPhone));
        assert_eq!(ContactField::from_name("Внутренний номер"), Some(ContactField::InternalPhone));
        assert_eq!(ContactField::from_name("Мобильный телефон"), Some(ContactField::MobilePhone));
        assert_eq!(ContactField::from_name("Email"), Some(ContactField::Email));
        assert_eq!(ContactField::from_name("Электронная почта"), Some(ContactField::Email));
        assert_eq!(ContactField::from_name("Этаж"), Some(ContactField::Floor));
        assert_eq!(ContactField::from_name("Этаж / Корпус"), Some(ContactField::Floor));
        assert_eq!(ContactField::from_name("Кабинет"), Some(ContactField::Office));
        assert_eq!(ContactField::from_name("Кабинет / Номер кабинета"), Some(ContactField::Office));
        assert_eq!(ContactField::from_name("Номер кабинета"), Some(ContactField::Office));

        // 3. SQL columns list
        let select_cols = FieldMapper::sql_select_columns();
        assert!(select_cols.contains("location"));
        assert!(select_cols.contains("department"));
        assert!(select_cols.contains("position"));
        assert!(select_cols.contains("full_name"));
        assert!(select_cols.contains("internal_phone"));
        assert!(select_cols.contains("mobile_phone"));
        assert!(select_cols.contains("email"));
        assert!(select_cols.contains("floor"));
        assert!(select_cols.contains("office"));

        let insert_cols = FieldMapper::sql_insert_columns();
        assert!(!insert_cols.split(", ").any(|c| c == "id"));
        assert!(insert_cols.contains("office"));
        assert!(insert_cols.contains("is_hidden"));

        // 4. Contact to Map and back
        let mut contact = Contact::new(
            "Главный офис".to_string(),
            "Разработка".to_string(),
            "Архитектор".to_string(),
            "Сидоров Алексей".to_string(),
            "3344".to_string(),
            "+7 900 123-45-67".to_string(),
            "sidorov@corp.org".to_string(),
            "5".to_string(),
            "512".to_string(),
        );
        contact.is_hidden = true;

        let map = FieldMapper::contact_to_map(&contact);
        assert_eq!(map.get(&ContactField::FullName).unwrap(), "Сидоров Алексей");
        assert_eq!(map.get(&ContactField::Office).unwrap(), "512");
        assert_eq!(map.get(&ContactField::IsHidden).unwrap(), "true");

        let reconstructed = FieldMapper::contact_from_map(&map);
        assert_eq!(reconstructed.full_name, "Сидоров Алексей");
        assert_eq!(reconstructed.office, "512");
        assert_eq!(reconstructed.room(), "512");
        assert!(reconstructed.is_hidden);
    }

    #[test]
    fn test_db_config_and_escape() {
        use corporate_contacts::db::{escape_sql, DbConfig};
        use corporate_contacts::settings::AppSettings;

        let mut settings = AppSettings::default();
        settings.db_host = "db.example.com".to_string();
        settings.db_port = "5433".to_string();
        settings.db_name = "testdb".to_string();
        settings.db_user = "testuser".to_string();
        settings.db_password = "secret'pass".to_string();

        let cfg = DbConfig::from_env_or_settings(&settings).expect("Config should be parsed");
        assert_eq!(cfg.host, "db.example.com");
        assert_eq!(cfg.port, 5433);
        assert_eq!(cfg.name, "testdb");
        assert_eq!(cfg.user, "testuser");
        assert_eq!(cfg.password, "secret'pass");

        let conninfo = cfg.to_conninfo();
        assert!(conninfo.contains("host='db.example.com'"));
        assert!(conninfo.contains("port=5433"));
        assert!(conninfo.contains("dbname='testdb'"));
        assert!(conninfo.contains("user='testuser'"));

        // SQL escaping
        assert_eq!(escape_sql("O'Connor"), "'O''Connor'");
        assert_eq!(escape_sql("Normal Text"), "'Normal Text'");
        assert_eq!(escape_sql("''double''"), "'''''double'''''");
    }

    #[test]
    fn test_backward_compatibility_room_alias() {
        // Test parsing JSON containing legacy 'room' field
        let legacy_json = r#"{"id":"10","location":"Офис","department":"ИТ","position":"Инженер","full_name":"Петров П.П.","internal_phone":"101","mobile_phone":"+7","email":"p@c.org","floor":"1","room":"202"}"#;
        let c: Contact = serde_json::from_str(legacy_json).expect("Failed to deserialize legacy JSON");
        assert_eq!(c.office, "202");
        assert_eq!(c.room(), "202");

        // Test parsing JSON containing modern 'office' field
        let modern_json = r#"{"id":"11","location":"Офис","department":"ИТ","position":"Инженер","full_name":"Петров П.П.","internal_phone":"101","mobile_phone":"+7","email":"p@c.org","floor":"1","office":"303"}"#;
        let c2: Contact = serde_json::from_str(modern_json).expect("Failed to deserialize modern JSON");
        assert_eq!(c2.office, "303");
        assert_eq!(c2.room(), "303");
    }
}

