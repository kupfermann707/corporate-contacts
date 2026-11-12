use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

fn default_org_name() -> String {
    "Справочник контактов".to_string()
}

fn default_location_name() -> String {
    "Все".to_string()
}

fn default_envelope_notice() -> String {
    "Файл зашифрован для защиты учетных данных и паролей БД".to_string()
}

pub fn default_https_enabled() -> bool {
    true
}

pub fn default_tls_cert_path() -> String {
    if let Some(mut cfg) = dirs::config_dir() {
        cfg.push("corporate-contacts");
        cfg.push("certs");
        cfg.join("cert.pem").to_string_lossy().to_string()
    } else {
        "cert.pem".to_string()
    }
}

pub fn default_tls_key_path() -> String {
    if let Some(mut cfg) = dirs::config_dir() {
        cfg.push("corporate-contacts");
        cfg.push("certs");
        cfg.join("key.pem").to_string_lossy().to_string()
    } else {
        "key.pem".to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub web_server_enabled: bool,
    pub web_server_port: u16,
    #[serde(default = "default_https_enabled")]
    pub https_enabled: bool,
    #[serde(default = "default_tls_cert_path")]
    pub tls_cert_path: String,
    #[serde(default = "default_tls_key_path")]
    pub tls_key_path: String,
    pub theme: String, // "system", "light", "dark"
    #[serde(default = "default_org_name")]
    pub organization_name: String,
    #[serde(default = "default_location_name")]
    pub default_location: String,
    #[serde(default)]
    pub db_host: String,
    #[serde(default)]
    pub db_port: String,
    #[serde(default)]
    pub db_name: String,
    #[serde(default)]
    pub db_user: String,
    #[serde(default)]
    pub db_password: String,
    #[serde(default)]
    pub totp_enabled: bool,
    #[serde(default)]
    pub totp_secret: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct EncryptedEnvelope {
    #[serde(default = "default_envelope_notice")]
    pub _notice: String,
    pub encrypted: bool,
    pub version: u32,
    pub iv: String,
    pub tag: String,
    pub payload: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            web_server_enabled: false,
            web_server_port: 8443,
            https_enabled: true,
            tls_cert_path: default_tls_cert_path(),
            tls_key_path: default_tls_key_path(),
            theme: "system".to_string(),
            organization_name: default_org_name(),
            default_location: default_location_name(),
            db_host: String::new(),
            db_port: String::new(),
            db_name: "postgres".to_string(),
            db_user: "postgres".to_string(),
            db_password: String::new(),
            totp_enabled: false,
            totp_secret: String::new(),
        }
    }
}

impl AppSettings {
    pub fn is_db_configured(&self) -> bool {
        let env_host = std::env::var("DB_HOST").ok().unwrap_or_default();
        let env_port = std::env::var("DB_PORT").ok().unwrap_or_default();
        let host = if !env_host.trim().is_empty() {
            env_host
        } else {
            self.db_host.clone()
        };
        let port = if !env_port.trim().is_empty() {
            env_port
        } else {
            self.db_port.clone()
        };
        !host.trim().is_empty() && !port.trim().is_empty()
    }

    pub fn config_path() -> PathBuf {
        if let Some(mut cfg) = dirs::config_dir() {
            cfg.push("corporate-contacts");
            let _ = fs::create_dir_all(&cfg);
            cfg.join("settings.json")
        } else {
            PathBuf::from("settings.json")
        }
    }

    pub fn load() -> Self {
        Self::load_from_path(&Self::config_path())
    }

    pub fn load_from_path(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }

        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("Не удалось прочитать настройки из {}: {}", path.display(), e);
                return Self::default();
            }
        };

        // 1. Try parsing as encrypted envelope
        if let Ok(envelope) = serde_json::from_slice::<EncryptedEnvelope>(&bytes) {
            if envelope.encrypted {
                let key = crate::crypto::derive_machine_key();
                let iv = match crate::crypto::from_hex(&envelope.iv) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("Ошибка разбора IV настроек: {}", e);
                        return Self::default();
                    }
                };
                let tag = match crate::crypto::from_hex(&envelope.tag) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("Ошибка разбора Tag настроек: {}", e);
                        return Self::default();
                    }
                };
                let ciphertext = match crate::crypto::from_hex(&envelope.payload) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("Ошибка разбора payload настроек: {}", e);
                        return Self::default();
                    }
                };

                match crate::crypto::decrypt_data(&ciphertext, &iv, &tag, &key) {
                    Ok(plaintext) => {
                        if let Ok(settings) = serde_json::from_slice::<AppSettings>(&plaintext) {
                            return settings;
                        } else {
                            eprintln!("Ошибка десериализации расшифрованных настроек");
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "Ошибка расшифровки {}: {}. Создана резервная копия .bak",
                            path.display(),
                            e
                        );
                        let _ = fs::copy(path, path.with_extension("json.bak"));
                    }
                }
                return Self::default();
            }
        }

        // 2. Backward compatibility: legacy plaintext JSON (e.g. initial setup)
        if let Ok(settings) = serde_json::from_slice::<AppSettings>(&bytes) {
            // Automatically migrate and re-save in encrypted format
            let _ = settings.save_to_path(path);
            return settings;
        }

        Self::default()
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_to_path(&Self::config_path())
    }

    pub fn save_to_path(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // Serialize inner settings
        let inner_json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Ошибка сериализации настроек: {}", e))?;

        // Encrypt with machine key
        let key = crate::crypto::derive_machine_key();
        let (iv, tag, ciphertext) = crate::crypto::encrypt_data(inner_json.as_bytes(), &key);

        let envelope = EncryptedEnvelope {
            _notice: default_envelope_notice(),
            encrypted: true,
            version: 1,
            iv: crate::crypto::to_hex(&iv),
            tag: crate::crypto::to_hex(&tag),
            payload: crate::crypto::to_hex(&ciphertext),
        };

        let final_json = serde_json::to_string_pretty(&envelope)
            .map_err(|e| format!("Ошибка сериализации зашифрованного конверта: {}", e))?;

        fs::write(path, final_json)
            .map_err(|e| format!("Не удалось сохранить настройки {}: {}", path.display(), e))?;

        // Ensure restricted permissions (0600: read/write owner only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
        }

        Ok(())
    }

    pub fn apply_theme(&self) {
        let style_manager = adw::StyleManager::default();
        match self.theme.as_str() {
            "light" => style_manager.set_color_scheme(adw::ColorScheme::ForceLight),
            "dark" => style_manager.set_color_scheme(adw::ColorScheme::ForceDark),
            _ => style_manager.set_color_scheme(adw::ColorScheme::Default),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypted_settings_save_and_load() {
        let temp_dir = std::env::temp_dir().join("corporate_contacts_test_enc");
        let _ = fs::create_dir_all(&temp_dir);
        let config_file = temp_dir.join("test_settings.json");

        let mut original = AppSettings::default();
        original.db_host = "192.168.1.100".to_string();
        original.db_port = "5432".to_string();
        original.db_user = "admin_user".to_string();
        original.db_password = "SuperSecretDbPassword2026!".to_string();

        original.save_to_path(&config_file).expect("Save failed");

        // Verify that raw file on disk is encrypted and does not leak password
        let raw_content = fs::read_to_string(&config_file).expect("Read raw file failed");
        assert!(raw_content.contains("\"encrypted\": true"));
        assert!(
            !raw_content.contains("SuperSecretDbPassword2026!"),
            "Database password must NOT be in plaintext on disk!"
        );
        assert!(
            !raw_content.contains("admin_user"),
            "Settings details should be encrypted!"
        );

        // Load back and verify values
        let loaded = AppSettings::load_from_path(&config_file);
        assert_eq!(loaded.db_host, "192.168.1.100");
        assert_eq!(loaded.db_port, "5432");
        assert_eq!(loaded.db_user, "admin_user");
        assert_eq!(loaded.db_password, "SuperSecretDbPassword2026!");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_legacy_plaintext_settings_migration() {
        let temp_dir = std::env::temp_dir().join("corporate_contacts_test_mig");
        let _ = fs::create_dir_all(&temp_dir);
        let config_file = temp_dir.join("legacy_settings.json");

        // Write plaintext JSON
        let plain_json = r#"{
            "web_server_enabled": true,
            "web_server_port": 9090,
            "theme": "dark",
            "organization_name": "ООО Рога и Копыта",
            "default_location": "Москва",
            "db_host": "db.corp.local",
            "db_port": "5433",
            "db_name": "corpdb",
            "db_user": "corpuser",
            "db_password": "OldPlaintextPassword!"
        }"#;
        fs::write(&config_file, plain_json).expect("Write legacy json failed");

        // Load should read it and automatically save back encrypted
        let loaded = AppSettings::load_from_path(&config_file);
        assert_eq!(loaded.db_password, "OldPlaintextPassword!");
        assert_eq!(loaded.db_host, "db.corp.local");
        assert_eq!(loaded.web_server_port, 9090);

        // Check that the file on disk is now encrypted!
        let updated_content = fs::read_to_string(&config_file).expect("Read updated file failed");
        assert!(updated_content.contains("\"encrypted\": true"));
        assert!(
            !updated_content.contains("OldPlaintextPassword!"),
            "Plaintext password must be replaced by encrypted content!"
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
