use crate::model::Contact;
use std::fs;
use std::path::{Path, PathBuf};

pub struct StorageManager {
    file_path: PathBuf,
}

impl StorageManager {
    pub fn new() -> Self {
        let file_path = Self::resolve_data_path();
        Self { file_path }
    }

    #[allow(dead_code)]
    pub fn data_path(&self) -> &Path {
        &self.file_path
    }

    /// Resolve the most appropriate data file path:
    /// 1. Current working directory: ./data.json
    /// 2. Local user data directory: ~/.local/share/corporate-contacts/data.json
    /// 3. System installed directory: /usr/share/corporate-contacts/data.json
    fn resolve_data_path() -> PathBuf {
        let local_path = PathBuf::from("data.json");
        if local_path.exists() {
            return local_path;
        }

        if let Some(mut data_dir) = dirs::data_dir() {
            data_dir.push("corporate-contacts");
            let user_file = data_dir.join("data.json");
            if user_file.exists() {
                return user_file;
            }
        }

        let sys_path = PathBuf::from("/usr/share/corporate-contacts/data.json");
        if sys_path.exists() {
            return sys_path;
        }

        if let Some(mut data_dir) = dirs::data_dir() {
            data_dir.push("corporate-contacts");
            let _ = fs::create_dir_all(&data_dir);
            return data_dir.join("data.json");
        }

        PathBuf::from("data.json")
    }

    /// Resolve active file path for reading contacts
    fn resolve_active_read_path(&self) -> PathBuf {
        let local_path = PathBuf::from("data.json");
        if local_path.exists() {
            return local_path;
        }

        if let Some(mut data_dir) = dirs::data_dir() {
            data_dir.push("corporate-contacts");
            let user_file = data_dir.join("data.json");
            if user_file.exists() {
                return user_file;
            }
        }

        let sys_path = PathBuf::from("/usr/share/corporate-contacts/data.json");
        if sys_path.exists() {
            return sys_path;
        }

        self.file_path.clone()
    }

    /// Resolve active file path for writing contacts
    fn resolve_active_write_path(&self) -> PathBuf {
        let local_path = PathBuf::from("data.json");
        if local_path.exists() {
            return local_path;
        }

        if let Some(mut data_dir) = dirs::data_dir() {
            data_dir.push("corporate-contacts");
            let _ = fs::create_dir_all(&data_dir);
            return data_dir.join("data.json");
        }

        PathBuf::from("data.json")
    }

    /// Load contacts from JSON file
    pub fn load_contacts(&self) -> Result<Vec<Contact>, String> {
        let path = self.resolve_active_read_path();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let bytes = fs::read(&path).map_err(|e| format!("Не удалось прочитать файл {}: {}", path.display(), e))?;
        
        // Strip UTF-8 BOM if present
        let content_str = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
            String::from_utf8_lossy(&bytes[3..]).to_string()
        } else {
            String::from_utf8_lossy(&bytes).to_string()
        };

        let trimmed = content_str.trim();
        if trimmed.is_empty() || trimmed == "[]" {
            return Ok(Vec::new());
        }

        // 1. Try direct deserialization into Vec<Contact> (modern universal format)
        if let Ok(mut contacts) = serde_json::from_str::<Vec<Contact>>(trimmed) {
            contacts.sort_by(|a, b| a.full_name.to_lowercase().cmp(&b.full_name.to_lowercase()));
            return Ok(contacts);
        }

        // 2. Fallback: Parse legacy dictionary structure (SheetName -> Array of rows)
        let raw_data: serde_json::Value = serde_json::from_str(trimmed)
            .map_err(|e| format!("Ошибка разбора JSON в {}: {}", path.display(), e))?;

        let mut contacts = Vec::new();

        if let Some(obj) = raw_data.as_object() {
            for (key, val) in obj {
                let loc_name = key.trim();

                let mut current_department = "Общий отдел".to_string();

                if let Some(rows) = val.as_array() {
                    for row in rows {
                        let cols: Vec<String> = if let Some(items) = row.as_array() {
                            items.iter().map(|item| {
                                item.as_str().unwrap_or("").trim().to_string()
                            }).collect()
                        } else {
                            continue;
                        };

                        let col1 = cols.get(0).cloned().unwrap_or_default();
                        let col2 = cols.get(1).cloned().unwrap_or_default();
                        let col3 = cols.get(2).cloned().unwrap_or_default();
                        let col4 = cols.get(3).cloned().unwrap_or_default();
                        let col5 = cols.get(4).cloned().unwrap_or_default();
                        let col6 = cols.get(5).cloned().unwrap_or_default();
                        let col7 = cols.get(6).cloned().unwrap_or_default();

                        if col1.is_empty() && col2.is_empty() && col3.is_empty() && col4.is_empty() && col5.is_empty() {
                            continue;
                        }

                        let c1_low = col1.to_lowercase();
                        let c2_low = col2.to_lowercase();

                        // Header detection for department
                        if col2.is_empty()
                            && !col1.is_empty()
                            && c1_low != "должность"
                            && !c1_low.contains("телефон")
                            && !c1_low.contains("список контактов")
                        {
                            current_department = col1;
                            continue;
                        }

                        // Contact row detection
                        if !col2.is_empty()
                            && c2_low != "фио"
                            && c2_low != "ф.и.о."
                            && !c2_low.contains("согласование счетов")
                        {
                            contacts.push(Contact::new(
                                loc_name.to_string(),
                                current_department.clone(),
                                col1,
                                col2,
                                col3,
                                col4,
                                col5,
                                col6,
                                col7,
                            ));
                        }
                    }
                }
            }
        }

        // Sort contacts by full name
        contacts.sort_by(|a, b| a.full_name.to_lowercase().cmp(&b.full_name.to_lowercase()));

        Ok(contacts)
    }

    /// Save contacts directly to clean JSON format
    pub fn save_contacts(&self, contacts: &[Contact]) -> Result<(), String> {
        let target_path = self.resolve_active_write_path();
        if let Some(parent) = target_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let formatted = serde_json::to_string_pretty(contacts)
            .map_err(|e| format!("Ошибка форматирования JSON: {}", e))?;

        fs::write(&target_path, formatted)
            .map_err(|e| format!("Не удалось сохранить файл {}: {}", target_path.display(), e))?;

        Ok(())
    }

    /// Export contacts to CSV file
    pub fn export_csv(&self, contacts: &[Contact], target_path: &Path) -> Result<(), String> {
        let mut csv = String::new();
        // UTF-8 BOM for Microsoft Excel compatibility
        csv.push('\u{feff}');
        let headers = crate::field_mapping::FieldMapper::csv_export_headers().join(";");
        csv.push_str(&headers);
        csv.push_str("\r\n");

        for c in contacts {
            let row = format!(
                "\"{}\";\"{}\";\"{}\";\"{}\";\"{}\";\"{}\";\"{}\";\"{}\";\"{}\"\r\n",
                c.location.replace('"', "\"\""),
                c.department.replace('"', "\"\""),
                c.position.replace('"', "\"\""),
                c.full_name.replace('"', "\"\""),
                c.internal_phone.replace('"', "\"\""),
                c.mobile_phone.replace('"', "\"\""),
                c.email.replace('"', "\"\""),
                c.floor.replace('"', "\"\""),
                c.office.replace('"', "\"\"")
            );
            csv.push_str(&row);
        }

        fs::write(target_path, csv)
            .map_err(|e| format!("Не удалось записать CSV {}: {}", target_path.display(), e))?;

        Ok(())
    }

    /// Parse CSV file and return contacts with a report of recognized columns
    pub fn parse_csv_file(path: &Path) -> Result<(Vec<Contact>, CsvImportReport), String> {
        let bytes = fs::read(path)
            .map_err(|e| format!("Не удалось прочитать файл {}: {}", path.display(), e))?;

        let content = Self::decode_text(&bytes);
        if content.trim().is_empty() {
            return Err("Файл пуст".to_string());
        }

        Self::parse_csv_content(&content)
    }

    /// Decode raw bytes to string handling UTF-8 (with or without BOM) and CP1251 fallback
    pub fn decode_text(bytes: &[u8]) -> String {
        let slice = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
            &bytes[3..]
        } else {
            bytes
        };

        if let Ok(s) = std::str::from_utf8(slice) {
            return s.to_string();
        }

        // Fallback for Windows-1251 encoded CSV files from Excel
        let mut out = String::with_capacity(slice.len());
        for &b in slice {
            match b {
                0x00..=0x7F => out.push(b as char),
                0xA8 => out.push('Ё'),
                0xB8 => out.push('ё'),
                0xB9 => out.push('№'),
                0xC0..=0xFF => {
                    let cp = 0x0410 + (b - 0xC0) as u32;
                    if let Some(ch) = char::from_u32(cp) {
                        out.push(ch);
                    } else {
                        out.push('?');
                    }
                }
                _ => out.push('?'),
            }
        }
        out
    }

    /// Parse CSV content into contacts
    pub fn parse_csv_content(content: &str) -> Result<(Vec<Contact>, CsvImportReport), String> {
        let content = content.strip_prefix('\u{feff}').unwrap_or(content);
        let delimiter = Self::detect_delimiter(content);
        let records = Self::parse_csv_records(content, delimiter);

        if records.is_empty() {
            return Err("Не удалось распознать строки данных в CSV файле".to_string());
        }

        let first_row = &records[0];
        let (col_indices, is_header, recognized_cols) = Self::detect_columns(first_row);

        let data_rows = if is_header {
            &records[1..]
        } else {
            &records[..]
        };

        let mut contacts = Vec::new();
        let mut current_dept = "Общий отдел".to_string();

        for row in data_rows {
            if row.is_empty() || row.iter().all(|c| c.trim().is_empty()) {
                continue;
            }

            let full_name = col_indices
                .full_name
                .and_then(|idx| row.get(idx))
                .cloned()
                .unwrap_or_default()
                .trim()
                .to_string();

            let dept_val = col_indices
                .department
                .and_then(|idx| row.get(idx))
                .cloned()
                .unwrap_or_default()
                .trim()
                .to_string();

            if !dept_val.is_empty() {
                current_dept = dept_val.clone();
            }

            // If name is empty, this could be a department section row
            if full_name.is_empty() {
                let first_col = row.get(0).map(|s| s.trim()).unwrap_or("");
                if !first_col.is_empty() && row.iter().skip(1).all(|s| s.trim().is_empty()) {
                    current_dept = first_col.to_string();
                }
                continue;
            }

            // Skip accidental repeated headers
            let name_low = full_name.to_lowercase();
            if name_low == "фио" || name_low == "ф.и.о." || name_low == "имя" || name_low == "full name" {
                continue;
            }

            let location_val = col_indices
                .location
                .and_then(|idx| row.get(idx))
                .cloned()
                .unwrap_or_default()
                .trim()
                .to_string();

            let location = location_val;

            let department = if dept_val.is_empty() {
                current_dept.clone()
            } else {
                dept_val
            };

            let position = col_indices
                .position
                .and_then(|idx| row.get(idx))
                .cloned()
                .unwrap_or_default()
                .trim()
                .to_string();

            let internal_phone = col_indices
                .internal_phone
                .and_then(|idx| row.get(idx))
                .cloned()
                .unwrap_or_default()
                .trim()
                .to_string();

            let mobile_phone = col_indices
                .mobile_phone
                .and_then(|idx| row.get(idx))
                .cloned()
                .unwrap_or_default()
                .trim()
                .to_string();

            let email = col_indices
                .email
                .and_then(|idx| row.get(idx))
                .cloned()
                .unwrap_or_default()
                .trim()
                .to_string();

            let floor = col_indices
                .floor
                .and_then(|idx| row.get(idx))
                .cloned()
                .unwrap_or_default()
                .trim()
                .to_string();

            let office = col_indices
                .office
                .and_then(|idx| row.get(idx))
                .cloned()
                .unwrap_or_default()
                .trim()
                .to_string();

            contacts.push(Contact::new(
                location,
                department,
                position,
                full_name,
                internal_phone,
                mobile_phone,
                email,
                floor,
                office,
            ));
        }

        let report = CsvImportReport {
            total_read: contacts.len(),
            recognized_columns: recognized_cols,
            delimiter,
        };

        Ok((contacts, report))
    }

    /// Detect delimiter from the first non-empty line
    pub fn detect_delimiter(content: &str) -> char {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let semicolons = trimmed.chars().filter(|&c| c == ';').count();
            let commas = trimmed.chars().filter(|&c| c == ',').count();
            let tabs = trimmed.chars().filter(|&c| c == '\t').count();

            if semicolons >= commas && semicolons >= tabs && semicolons > 0 {
                return ';';
            }
            if tabs > commas && tabs > semicolons && tabs > 0 {
                return '\t';
            }
            if commas > 0 {
                return ',';
            }
            break;
        }
        ';'
    }

    /// Parse CSV records handling quotes and escaped quotes
    pub fn parse_csv_records(content: &str, delimiter: char) -> Vec<Vec<String>> {
        let mut records = Vec::new();
        let mut current_record = Vec::new();
        let mut current_field = String::new();
        let mut in_quotes = false;
        let mut chars = content.chars().peekable();

        while let Some(ch) = chars.next() {
            if in_quotes {
                if ch == '"' {
                    if chars.peek() == Some(&'"') {
                        // Escaped quote: "" -> "
                        chars.next();
                        current_field.push('"');
                    } else {
                        in_quotes = false;
                    }
                } else {
                    current_field.push(ch);
                }
            } else {
                if ch == '"' {
                    in_quotes = true;
                } else if ch == delimiter {
                    current_record.push(current_field.trim().to_string());
                    current_field.clear();
                } else if ch == '\n' {
                    current_record.push(current_field.trim().to_string());
                    current_field.clear();
                    if !current_record.iter().all(|s| s.is_empty()) {
                        records.push(current_record);
                    }
                    current_record = Vec::new();
                } else if ch == '\r' {
                    // Ignore CR
                } else {
                    current_field.push(ch);
                }
            }
        }

        if !current_field.is_empty() || !current_record.is_empty() {
            current_record.push(current_field.trim().to_string());
            if !current_record.iter().all(|s| s.is_empty()) {
                records.push(current_record);
            }
        }

        records
    }

    /// Detect which column index corresponds to which contact field
    pub fn detect_columns(header: &[String]) -> (CsvColumnIndices, bool, Vec<String>) {
        let mut indices = CsvColumnIndices::default();
        let mut recognized = Vec::new();

        for (i, col) in header.iter().enumerate() {
            if let Some(field) = crate::field_mapping::ContactField::from_name(col) {
                match field {
                    crate::field_mapping::ContactField::FullName if indices.full_name.is_none() => {
                        indices.full_name = Some(i);
                        recognized.push(field.russian_header().to_string());
                    }
                    crate::field_mapping::ContactField::Location if indices.location.is_none() => {
                        indices.location = Some(i);
                        recognized.push(field.russian_header().to_string());
                    }
                    crate::field_mapping::ContactField::Department if indices.department.is_none() => {
                        indices.department = Some(i);
                        recognized.push(field.russian_header().to_string());
                    }
                    crate::field_mapping::ContactField::Position if indices.position.is_none() => {
                        indices.position = Some(i);
                        recognized.push(field.russian_header().to_string());
                    }
                    crate::field_mapping::ContactField::InternalPhone if indices.internal_phone.is_none() => {
                        indices.internal_phone = Some(i);
                        recognized.push(field.russian_header().to_string());
                    }
                    crate::field_mapping::ContactField::MobilePhone if indices.mobile_phone.is_none() => {
                        indices.mobile_phone = Some(i);
                        recognized.push(field.russian_header().to_string());
                    }
                    crate::field_mapping::ContactField::Email if indices.email.is_none() => {
                        indices.email = Some(i);
                        recognized.push(field.russian_header().to_string());
                    }
                    crate::field_mapping::ContactField::Floor if indices.floor.is_none() => {
                        indices.floor = Some(i);
                        recognized.push(field.russian_header().to_string());
                    }
                    crate::field_mapping::ContactField::Office if indices.office.is_none() => {
                        indices.office = Some(i);
                        recognized.push(field.russian_header().to_string());
                    }
                    _ => {}
                }
            }
        }

        let is_header = indices.full_name.is_some() || recognized.len() >= 2;

        if !is_header {
            let col_count = header.len();
            recognized.clear();
            if col_count >= 9 {
                indices = CsvColumnIndices {
                    location: Some(0),
                    department: Some(1),
                    position: Some(2),
                    full_name: Some(3),
                    internal_phone: Some(4),
                    mobile_phone: Some(5),
                    email: Some(6),
                    floor: Some(7),
                    office: Some(8),
                };
                recognized = vec![
                    "Локация (кол. 1)".to_string(),
                    "Отдел (кол. 2)".to_string(),
                    "Должность (кол. 3)".to_string(),
                    "ФИО (кол. 4)".to_string(),
                    "Внутренний телефон (кол. 5)".to_string(),
                    "Мобильный телефон (кол. 6)".to_string(),
                    "Email (кол. 7)".to_string(),
                    "Этаж / Корпус (кол. 8)".to_string(),
                    "Кабинет (кол. 9)".to_string(),
                ];
            } else if col_count == 7 {
                indices = CsvColumnIndices {
                    location: None,
                    department: None,
                    position: Some(0),
                    full_name: Some(1),
                    internal_phone: Some(2),
                    mobile_phone: Some(3),
                    email: Some(4),
                    floor: Some(5),
                    office: Some(6),
                };
                recognized = vec![
                    "Должность (кол. 1)".to_string(),
                    "ФИО (кол. 2)".to_string(),
                    "Внутренний телефон (кол. 3)".to_string(),
                    "Мобильный телефон (кол. 4)".to_string(),
                    "Email (кол. 5)".to_string(),
                    "Этаж / Корпус (кол. 6)".to_string(),
                    "Кабинет (кол. 7)".to_string(),
                ];
            } else {
                indices = CsvColumnIndices {
                    location: None,
                    department: if col_count > 2 { Some(1) } else { None },
                    position: if col_count > 3 { Some(2) } else { None },
                    full_name: Some(0),
                    internal_phone: if col_count > 4 { Some(3) } else { None },
                    mobile_phone: if col_count > 1 { Some(if col_count > 5 { 4 } else { 1 }) } else { None },
                    email: if col_count > 6 { Some(5) } else { None },
                    floor: if col_count > 7 { Some(6) } else { None },
                    office: if col_count > 8 { Some(7) } else { None },
                };
                recognized.push("ФИО (кол. 1)".to_string());
                if col_count > 1 {
                    recognized.push("Телефон (кол. 2)".to_string());
                }
            }
        }

        (indices, is_header, recognized)
    }

    /// Merge imported contacts into existing list:
    /// - If contact with same full_name and department already exists, updates its details.
    /// - Otherwise appends the new contact.
    pub fn merge_contacts(existing: &mut Vec<Contact>, imported: Vec<Contact>) -> usize {
        let mut count = 0;
        for imp in imported {
            if let Some(pos) = existing.iter().position(|c| {
                c.full_name.trim().to_lowercase() == imp.full_name.trim().to_lowercase()
                    && c.department.trim().to_lowercase() == imp.department.trim().to_lowercase()
            }) {
                if !imp.position.is_empty() {
                    existing[pos].position = imp.position;
                }
                if !imp.internal_phone.is_empty() {
                    existing[pos].internal_phone = imp.internal_phone;
                }
                if !imp.mobile_phone.is_empty() {
                    existing[pos].mobile_phone = imp.mobile_phone;
                }
                if !imp.email.is_empty() {
                    existing[pos].email = imp.email;
                }
                if !imp.floor.is_empty() {
                    existing[pos].floor = imp.floor;
                }
                if !imp.office.is_empty() {
                    existing[pos].office = imp.office;
                }
                if !imp.location.is_empty() {
                    existing[pos].location = imp.location;
                }
            } else {
                existing.push(imp);
            }
            count += 1;
        }
        existing.sort_by(|a, b| a.full_name.to_lowercase().cmp(&b.full_name.to_lowercase()));
        count
    }
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct CsvImportReport {
    pub total_read: usize,
    pub recognized_columns: Vec<String>,
    pub delimiter: char,
}

#[derive(Debug, Default, Clone)]
#[allow(dead_code)]
pub struct CsvColumnIndices {
    pub location: Option<usize>,
    pub department: Option<usize>,
    pub position: Option<usize>,
    pub full_name: Option<usize>,
    pub internal_phone: Option<usize>,
    pub mobile_phone: Option<usize>,
    pub email: Option<usize>,
    pub floor: Option<usize>,
    pub office: Option<usize>,
}

