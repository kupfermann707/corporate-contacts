use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static AUDIT_LOCK: Mutex<()> = Mutex::new(());

/// Журнал аудита событий безопасности и доступа к персональным данным (согласно 152-ФЗ и приказу ФСТЭК № 21)
pub struct AuditLogger;

impl AuditLogger {
    /// Определение защищённого пути к файлу журнала аудита
    pub fn log_path() -> PathBuf {
        if let Ok(custom) = std::env::var("CONTACTS_AUDIT_LOG_PATH") {
            return PathBuf::from(custom);
        }

        let var_log = PathBuf::from("/var/log/corporate-contacts/audit.log");
        if let Some(parent) = var_log.parent() {
            if parent.exists() {
                if let Ok(f) = OpenOptions::new().create(true).append(true).open(&var_log) {
                    drop(f);
                    return var_log;
                }
            }
        }

        if let Some(mut dir) = dirs::data_local_dir() {
            dir.push("corporate-contacts");
            if fs::create_dir_all(&dir).is_ok() {
                return dir.join("audit.log");
            }
        }

        if let Some(mut cfg) = dirs::config_dir() {
            cfg.push("corporate-contacts");
            if fs::create_dir_all(&cfg).is_ok() {
                return cfg.join("audit.log");
            }
        }

        // Резервный путь во временную директорию при ограниченных правах песочницы/контейнера
        let tmp = std::env::temp_dir().join("corporate-contacts");
        let _ = fs::create_dir_all(&tmp);
        tmp.join("audit.log")
    }

    /// Запись события в журнал аудита по конкретному пути
    pub fn log_to_path(path: &Path, event_type: &str, user_or_ip: &str, details: &str) {
        let _guard = AUDIT_LOCK.lock().unwrap();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let timestamp = Self::iso_timestamp();
        let line = format!(
            "{} [152-FZ-AUDIT] [{}] [{}] {}\n",
            timestamp, event_type, user_or_ip, details
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .mode(0o600)
                .open(path)
            {
                let _ = file.write_all(line.as_bytes());
                let _ = file.flush();
            }
        }
        #[cfg(not(unix))]
        {
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = file.write_all(line.as_bytes());
                let _ = file.flush();
            }
        }
    }

    /// Запись события в журнал аудита по умолчанию
    pub fn log(event_type: &str, user_or_ip: &str, details: &str) {
        Self::log_to_path(&Self::log_path(), event_type, user_or_ip, details);
    }

    fn iso_timestamp() -> String {
        if let Ok(dt) = glib::DateTime::now_local() {
            if let Ok(s) = dt.format_iso8601() {
                return s.to_string();
            }
        }
        "1970-01-01T00:00:00Z".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_log_write() {
        let temp_dir = std::env::temp_dir().join("corporate_contacts_audit_test");
        let test_log = temp_dir.join("test_audit.log");
        let _ = std::fs::remove_file(&test_log);

        AuditLogger::log_to_path(&test_log, "TEST_EVENT", "127.0.0.1", "Тестовая запись журнала аудита 152-ФЗ");
        assert!(test_log.exists());
        let content = std::fs::read_to_string(&test_log).unwrap_or_default();
        assert!(content.contains("[152-FZ-AUDIT]"));
        assert!(content.contains("TEST_EVENT"));
        assert!(content.contains("Тестовая запись журнала аудита 152-ФЗ"));

        let _ = std::fs::remove_file(&test_log);
    }
}
