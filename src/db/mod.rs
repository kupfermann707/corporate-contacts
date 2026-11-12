pub mod libpq;
pub mod wire;

use crate::field_mapping::{ContactField, FieldMapper};
use crate::model::Contact;
use crate::settings::AppSettings;
use libpq::{LibPqClient, LibPqLoader};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wire::WireClient;

#[derive(Debug, Clone)]
pub struct DbConfig {
    pub host: String,
    pub port: u16,
    pub name: String,
    pub user: String,
    pub password: String,
}

impl DbConfig {
    /// Получение настроек подключения с приоритетом: Переменные окружения -> Настройки приложения
    pub fn from_env_or_settings(settings: &AppSettings) -> Option<Self> {
        let host = std::env::var("DB_HOST").ok().unwrap_or_else(|| settings.db_host.clone());
        let port_str = std::env::var("DB_PORT").ok().unwrap_or_else(|| settings.db_port.clone());
        let name = std::env::var("DB_NAME").ok().unwrap_or_else(|| settings.db_name.clone());
        let user = std::env::var("DB_USER").ok().unwrap_or_else(|| settings.db_user.clone());
        let password = std::env::var("DB_PASSWORD").ok().unwrap_or_else(|| settings.db_password.clone());

        let host = host.trim();
        let port_str = port_str.trim();

        if host.is_empty() || port_str.is_empty() {
            return None;
        }

        let port = port_str.parse::<u16>().unwrap_or(5432);
        let name = if name.trim().is_empty() {
            "postgres".to_string()
        } else {
            name.trim().to_string()
        };
        let user = if user.trim().is_empty() {
            "postgres".to_string()
        } else {
            user.trim().to_string()
        };

        Some(Self {
            host: host.to_string(),
            port,
            name,
            user,
            password,
        })
    }

    pub fn to_conninfo(&self) -> String {
        let mut parts = Vec::new();
        parts.push(format!("host='{}'", self.host.replace('\'', "\\'")));
        parts.push(format!("port={}", self.port));
        parts.push(format!("dbname='{}'", self.name.replace('\'', "\\'")));
        parts.push(format!("user='{}'", self.user.replace('\'', "\\'")));
        if !self.password.is_empty() {
            parts.push(format!("password='{}'", self.password.replace('\'', "\\'")));
        }
        parts.push("connect_timeout=5".to_string());
        parts.join(" ")
    }
}

enum Backend {
    LibPq(LibPqClient),
    Wire(WireClient),
}

impl Backend {
    fn execute(&mut self, query: &str) -> Result<wire::QueryResult, String> {
        match self {
            Backend::LibPq(client) => client.execute(query),
            Backend::Wire(client) => client.execute(query),
        }
    }
}

/// Высокоуровневый клиент базы данных PostgreSQL
#[derive(Clone)]
pub struct Database {
    backend: Arc<Mutex<Backend>>,
    config: DbConfig,
}

impl Database {
    /// Подключение к PostgreSQL с автовыбором: libpq -> pure wire
    pub fn connect(config: DbConfig) -> Result<Self, String> {
        // Попытка 1: Через нативную библиотеку libpq (поддерживает SCRAM-SHA-256, SSL, сокеты)
        if let Ok(loader) = LibPqLoader::try_load() {
            let conninfo = config.to_conninfo();
            if let Ok(client) = LibPqClient::connect(loader, &conninfo) {
                return Ok(Self {
                    backend: Arc::new(Mutex::new(Backend::LibPq(client))),
                    config,
                });
            }
        }

        // Попытка 2: Через прямой протокол PostgreSQL v3.0 (чистый Rust без системных библиотек)
        let wire = WireClient::connect(
            &config.host,
            config.port,
            &config.name,
            &config.user,
            &config.password,
        )?;

        Ok(Self {
            backend: Arc::new(Mutex::new(Backend::Wire(wire))),
            config,
        })
    }

    #[allow(dead_code)]
    pub fn config(&self) -> &DbConfig {
        &self.config
    }

    /// 1. Авто-инициализация базы (DDL)
    pub fn init_tables(&self) -> Result<(), String> {
        let ddl = "CREATE TABLE IF NOT EXISTS contacts (
    id SERIAL PRIMARY KEY,
    location VARCHAR(150),
    department VARCHAR(150),
    position VARCHAR(150),
    full_name VARCHAR(200) NOT NULL,
    internal_phone VARCHAR(50),
    mobile_phone VARCHAR(50),
    email VARCHAR(150),
    floor VARCHAR(50),
    office VARCHAR(50),
    is_hidden BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
ALTER TABLE contacts ADD COLUMN IF NOT EXISTS is_hidden BOOLEAN NOT NULL DEFAULT FALSE;";

        let mut b = self.backend.lock().map_err(|e| e.to_string())?;
        b.execute(ddl)?;
        Ok(())
    }

    /// Получение всех контактов из базы
    pub fn fetch_all(&self) -> Result<Vec<Contact>, String> {
        let sql = format!(
            "SELECT {} FROM contacts ORDER BY full_name ASC;",
            FieldMapper::sql_select_columns()
        );

        let mut b = self.backend.lock().map_err(|e| e.to_string())?;
        let res = b.execute(&sql)?;

        let mut contacts = Vec::with_capacity(res.rows.len());

        // Индексируем колонки по имени
        let mut col_map: HashMap<ContactField, usize> = HashMap::new();
        for (i, name) in res.columns.iter().enumerate() {
            if let Some(field) = ContactField::from_name(name) {
                col_map.insert(field, i);
            }
        }

        for row in res.rows {
            let mut field_values = HashMap::new();
            for field in ContactField::ALL {
                if let Some(&idx) = col_map.get(&field) {
                    if idx < row.len() {
                        field_values.insert(field, row[idx].clone());
                    }
                }
            }
            contacts.push(FieldMapper::contact_from_map(&field_values));
        }

        Ok(contacts)
    }

    /// Создание нового контакта: INSERT INTO contacts (...) VALUES (...) RETURNING id;
    pub fn insert_contact(&self, contact: &Contact) -> Result<String, String> {
        let sql = format!(
            "INSERT INTO contacts ({}) VALUES ({}, {}, {}, {}, {}, {}, {}, {}, {}, {}) RETURNING id;",
            FieldMapper::sql_insert_columns(),
            escape_sql(&contact.location),
            escape_sql(&contact.department),
            escape_sql(&contact.position),
            escape_sql(&contact.full_name),
            escape_sql(&contact.internal_phone),
            escape_sql(&contact.mobile_phone),
            escape_sql(&contact.email),
            escape_sql(&contact.floor),
            escape_sql(&contact.office),
            if contact.is_hidden { "TRUE" } else { "FALSE" },
        );

        let mut b = self.backend.lock().map_err(|e| e.to_string())?;
        let res = b.execute(&sql)?;

        if let Some(row) = res.rows.first() {
            if let Some(id_str) = row.first() {
                return Ok(id_str.clone());
            }
        }

        Ok(contact.id.clone())
    }

    /// Редактирование контакта: UPDATE contacts SET ... WHERE id = :id;
    pub fn update_contact(&self, contact: &Contact) -> Result<(), String> {
        // Если ID числовой, используем id = X, иначе ищем по ФИО и отделу если id не число
        let id_cond = if let Ok(num) = contact.id.parse::<i64>() {
            format!("id = {}", num)
        } else {
            format!(
                "full_name = {} AND department = {}",
                escape_sql(&contact.full_name),
                escape_sql(&contact.department)
            )
        };

        let sql = format!(
            "UPDATE contacts SET location = {}, department = {}, position = {}, full_name = {}, \
             internal_phone = {}, mobile_phone = {}, email = {}, floor = {}, office = {}, \
             is_hidden = {}, updated_at = CURRENT_TIMESTAMP WHERE {};",
            escape_sql(&contact.location),
            escape_sql(&contact.department),
            escape_sql(&contact.position),
            escape_sql(&contact.full_name),
            escape_sql(&contact.internal_phone),
            escape_sql(&contact.mobile_phone),
            escape_sql(&contact.email),
            escape_sql(&contact.floor),
            escape_sql(&contact.office),
            if contact.is_hidden { "TRUE" } else { "FALSE" },
            id_cond
        );

        let mut b = self.backend.lock().map_err(|e| e.to_string())?;
        b.execute(&sql)?;
        Ok(())
    }

    /// Изменение статуса мягкого удаления (Soft Delete / Скрыть / Восстановить)
    pub fn set_hidden(&self, id: &str, is_hidden: bool) -> Result<(), String> {
        let id_cond = if let Ok(num) = id.parse::<i64>() {
            format!("id = {}", num)
        } else {
            format!("id = {}", escape_sql(id))
        };

        let sql = format!(
            "UPDATE contacts SET is_hidden = {}, updated_at = CURRENT_TIMESTAMP WHERE {};",
            if is_hidden { "TRUE" } else { "FALSE" },
            id_cond
        );

        let mut b = self.backend.lock().map_err(|e| e.to_string())?;
        b.execute(&sql)?;
        Ok(())
    }

    /// Инвертировать статус видимости (toggle-visibility)
    #[allow(dead_code)]
    pub fn toggle_hidden(&self, id: &str) -> Result<bool, String> {
        let id_cond = if let Ok(num) = id.parse::<i64>() {
            format!("id = {}", num)
        } else {
            format!("id = {}", escape_sql(id))
        };

        let sql = format!(
            "UPDATE contacts SET is_hidden = NOT is_hidden, updated_at = CURRENT_TIMESTAMP WHERE {} RETURNING is_hidden;",
            id_cond
        );

        let mut b = self.backend.lock().map_err(|e| e.to_string())?;
        let res = b.execute(&sql)?;
        if let Some(row) = res.rows.first() {
            if let Some(val) = row.first() {
                let h = val == "t" || val == "true" || val == "1";
                return Ok(h);
            }
        }
        Ok(false)
    }

    /// Удаление контакта: DELETE FROM contacts WHERE id = :id;
    #[allow(dead_code)]
    pub fn delete_contact(&self, contact: &Contact) -> Result<(), String> {
        let id_cond = if let Ok(num) = contact.id.parse::<i64>() {
            format!("id = {}", num)
        } else {
            format!(
                "full_name = {} AND department = {}",
                escape_sql(&contact.full_name),
                escape_sql(&contact.department)
            )
        };

        let sql = format!("DELETE FROM contacts WHERE {};", id_cond);
        let mut b = self.backend.lock().map_err(|e| e.to_string())?;
        b.execute(&sql)?;
        Ok(())
    }

    /// Пакетная заливка записей из CSV в таблицу contacts
    pub fn batch_insert_contacts(&self, contacts: &[Contact], replace: bool) -> Result<usize, String> {
        if contacts.is_empty() {
            return Ok(0);
        }

        let mut b = self.backend.lock().map_err(|e| e.to_string())?;

        // Транзакция
        b.execute("BEGIN;")?;

        if replace {
            // Очищаем таблицу перед заменой
            let _ = b.execute("TRUNCATE TABLE contacts RESTART IDENTITY;");
        }

        let chunk_size = 50;
        let mut total_inserted = 0;

        for chunk in contacts.chunks(chunk_size) {
            let mut values_parts = Vec::with_capacity(chunk.len());
            for c in chunk {
                values_parts.push(format!(
                    "({}, {}, {}, {}, {}, {}, {}, {}, {}, {})",
                    escape_sql(&c.location),
                    escape_sql(&c.department),
                    escape_sql(&c.position),
                    escape_sql(&c.full_name),
                    escape_sql(&c.internal_phone),
                    escape_sql(&c.mobile_phone),
                    escape_sql(&c.email),
                    escape_sql(&c.floor),
                    escape_sql(&c.office),
                    if c.is_hidden { "TRUE" } else { "FALSE" },
                ));
            }

            let sql = format!(
                "INSERT INTO contacts ({}) VALUES {};",
                FieldMapper::sql_insert_columns(),
                values_parts.join(", ")
            );

            if let Err(e) = b.execute(&sql) {
                let _ = b.execute("ROLLBACK;");
                return Err(format!("Ошибка batch insert: {}", e));
            }

            total_inserted += chunk.len();
        }

        b.execute("COMMIT;")?;
        Ok(total_inserted)
    }

    /// Полная очистка базы данных
    pub fn clear_all(&self) -> Result<(), String> {
        let mut b = self.backend.lock().map_err(|e| e.to_string())?;
        b.execute("TRUNCATE TABLE contacts RESTART IDENTITY;")?;
        Ok(())
    }
}

/// Экранирование строкового литерала для SQL
pub fn escape_sql(val: &str) -> String {
    let clean = val.replace('\'', "''");
    format!("'{}'", clean)
}
