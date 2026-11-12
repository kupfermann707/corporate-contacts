use crate::model::Contact;
use std::collections::HashMap;

/// Поля контакта с двусторонним маппингом: Английский SQL ↔ Русский UI/CSV
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContactField {
    Id,
    Location,
    Department,
    Position,
    FullName,
    InternalPhone,
    MobilePhone,
    Email,
    Floor,
    Office,
    IsHidden,
}

#[allow(dead_code)]
impl ContactField {
    /// Все поля контакта в каноническом порядке
    pub const ALL: [ContactField; 11] = [
        ContactField::Id,
        ContactField::Location,
        ContactField::Department,
        ContactField::Position,
        ContactField::FullName,
        ContactField::InternalPhone,
        ContactField::MobilePhone,
        ContactField::Email,
        ContactField::Floor,
        ContactField::Office,
        ContactField::IsHidden,
    ];

    /// Поля данных для вставки / отображения (без автоинкрементного ID)
    pub const DATA_FIELDS: [ContactField; 10] = [
        ContactField::Location,
        ContactField::Department,
        ContactField::Position,
        ContactField::FullName,
        ContactField::InternalPhone,
        ContactField::MobilePhone,
        ContactField::Email,
        ContactField::Floor,
        ContactField::Office,
        ContactField::IsHidden,
    ];

    /// Имя колонки в базе данных PostgreSQL (на английском языке)
    pub fn sql_column(&self) -> &'static str {
        match self {
            ContactField::Id => "id",
            ContactField::Location => "location",
            ContactField::Department => "department",
            ContactField::Position => "position",
            ContactField::FullName => "full_name",
            ContactField::InternalPhone => "internal_phone",
            ContactField::MobilePhone => "mobile_phone",
            ContactField::Email => "email",
            ContactField::Floor => "floor",
            ContactField::Office => "office",
            ContactField::IsHidden => "is_hidden",
        }
    }

    /// Основной русский заголовок для UI и экспорта в CSV
    pub fn russian_header(&self) -> &'static str {
        match self {
            ContactField::Id => "ID",
            ContactField::Location => "Локация / Филиал",
            ContactField::Department => "Отдел",
            ContactField::Position => "Должность",
            ContactField::FullName => "ФИО",
            ContactField::InternalPhone => "Внутренний телефон",
            ContactField::MobilePhone => "Мобильный телефон",
            ContactField::Email => "Email",
            ContactField::Floor => "Этаж / Корпус",
            ContactField::Office => "Кабинет",
            ContactField::IsHidden => "Скрыт",
        }
    }

    /// Краткий русский заголовок для CSV / UI
    pub fn russian_short(&self) -> &'static str {
        match self {
            ContactField::Id => "ID",
            ContactField::Location => "Локация",
            ContactField::Department => "Отдел",
            ContactField::Position => "Должность",
            ContactField::FullName => "ФИО",
            ContactField::InternalPhone => "Внутренний номер",
            ContactField::MobilePhone => "Мобильный телефон",
            ContactField::Email => "Электронная почта",
            ContactField::Floor => "Этаж",
            ContactField::Office => "Кабинет / Номер кабинета",
            ContactField::IsHidden => "Скрыт",
        }
    }

    /// Определение поля по имени (английская колонка SQL или любой русский вариант заголовка)
    pub fn from_name(name: &str) -> Option<ContactField> {
        let clean = name.trim_start_matches('\u{feff}').trim();
        if let Some(f) = Self::from_single_name(clean) {
            return Some(f);
        }
        if clean.contains('/') {
            for part in clean.split('/') {
                if let Some(f) = Self::from_single_name(part.trim()) {
                    return Some(f);
                }
            }
        }
        None
    }

    fn from_single_name(name: &str) -> Option<ContactField> {
        let clean = name
            .trim_start_matches('\u{feff}')
            .trim()
            .to_lowercase()
            .replace(['_', '-', '–', '—', '.'], " ");
        let words: Vec<&str> = clean.split_whitespace().collect();
        let normalized = words.join(" ");

        match normalized.as_str() {
            "id" | "код" | "идентификатор" => Some(ContactField::Id),
            
            "location" | "локация" | "филиал" | "локация филиал" | "город" | "офис компании" | "branch" => {
                Some(ContactField::Location)
            }
            
            "department" | "отдел" | "подразделение" | "департамент" | "служба" | "управление" | "dept" => {
                Some(ContactField::Department)
            }
            
            "position" | "должность" | "профессия" | "специальность" | "роль" | "title" | "job title" => {
                Some(ContactField::Position)
            }
            
            "full name" | "fullname" | "фио" | "сотрудник" | "имя" | "работник" | "фио сотрудника" | "name" => {
                Some(ContactField::FullName)
            }
            
            "internal phone" | "internalphone" | "внутренний телефон" | "внутренний номер" | "внутренний"
            | "внутр телефон" | "внутр номер" | "доб" | "добавочный" | "доб номер" | "ext" | "extension" => {
                Some(ContactField::InternalPhone)
            }
            
            "mobile phone" | "mobilephone" | "мобильный телефон" | "мобильный" | "сотовый" | "сотовый телефон"
            | "моб телефон" | "телефон" | "phone" | "mobile" | "cell" => {
                Some(ContactField::MobilePhone)
            }
            
            "email" | "e mail" | "электронная почта" | "почта" | "эл почта" | "mail" => {
                Some(ContactField::Email)
            }
            
            "floor" | "этаж" | "этаж корпус" | "корпус этаж" | "корпус" | "уровень" => {
                Some(ContactField::Floor)
            }
            
            "office" | "room" | "кабинет" | "номер кабинета" | "комната" | "каб" | "офис" => {
                Some(ContactField::Office)
            }

            "is_hidden" | "ishidden" | "hidden" | "скрыт" | "скрытый" | "на удаление" => {
                Some(ContactField::IsHidden)
            }
            
            _ => None,
        }
    }

    /// Получить значение поля из модели Contact
    pub fn get_value<'a>(&self, contact: &'a Contact) -> &'a str {
        match self {
            ContactField::Id => &contact.id,
            ContactField::Location => &contact.location,
            ContactField::Department => &contact.department,
            ContactField::Position => &contact.position,
            ContactField::FullName => &contact.full_name,
            ContactField::InternalPhone => &contact.internal_phone,
            ContactField::MobilePhone => &contact.mobile_phone,
            ContactField::Email => &contact.email,
            ContactField::Floor => &contact.floor,
            ContactField::Office => &contact.office,
            ContactField::IsHidden => if contact.is_hidden { "true" } else { "false" },
        }
    }

    /// Установить значение поля в модели Contact
    pub fn set_value(&self, contact: &mut Contact, value: String) {
        match self {
            ContactField::Id => contact.id = value,
            ContactField::Location => contact.location = value,
            ContactField::Department => contact.department = value,
            ContactField::Position => contact.position = value,
            ContactField::FullName => contact.full_name = value,
            ContactField::InternalPhone => contact.internal_phone = value,
            ContactField::MobilePhone => contact.mobile_phone = value,
            ContactField::Email => contact.email = value,
            ContactField::Floor => contact.floor = value,
            ContactField::Office => contact.office = value,
            ContactField::IsHidden => {
                let v = value.trim().to_lowercase();
                contact.is_hidden = v == "true" || v == "t" || v == "1" || v == "да";
            }
        }
    }
}

/// Адаптер / маппер для конвертации между сущностями
pub struct FieldMapper;

#[allow(dead_code)]
impl FieldMapper {
    /// Список колонок для SQL SELECT / INSERT
    pub fn sql_select_columns() -> &'static str {
        "id, location, department, position, full_name, internal_phone, mobile_phone, email, floor, office, is_hidden"
    }

    pub fn sql_insert_columns() -> &'static str {
        "location, department, position, full_name, internal_phone, mobile_phone, email, floor, office, is_hidden"
    }

    /// Заголовки для экспорта в CSV на русском языке
    pub fn csv_export_headers() -> Vec<&'static str> {
        vec![
            "Локация / Филиал",
            "Отдел",
            "Должность",
            "ФИО",
            "Внутренний телефон",
            "Мобильный телефон",
            "Email",
            "Этаж / Корпус",
            "Кабинет",
        ]
    }

    /// Создание контакта из мапы распознанных полей
    pub fn contact_from_map(fields: &HashMap<ContactField, String>) -> Contact {
        let id = fields.get(&ContactField::Id).cloned().unwrap_or_default();
        let location = fields.get(&ContactField::Location).cloned().unwrap_or_default();
        let department = fields.get(&ContactField::Department).cloned().unwrap_or_default();
        let position = fields.get(&ContactField::Position).cloned().unwrap_or_default();
        let full_name = fields.get(&ContactField::FullName).cloned().unwrap_or_default();
        let internal_phone = fields.get(&ContactField::InternalPhone).cloned().unwrap_or_default();
        let mobile_phone = fields.get(&ContactField::MobilePhone).cloned().unwrap_or_default();
        let email = fields.get(&ContactField::Email).cloned().unwrap_or_default();
        let floor = fields.get(&ContactField::Floor).cloned().unwrap_or_default();
        let office = fields.get(&ContactField::Office).cloned().unwrap_or_default();
        let is_hidden = fields
            .get(&ContactField::IsHidden)
            .map(|v| {
                let v = v.trim().to_lowercase();
                v == "true" || v == "t" || v == "1" || v == "да"
            })
            .unwrap_or(false);

        let final_id = if id.is_empty() {
            format!("{}_{}_{}_{}", location, department, full_name, glib::uuid_string_random())
        } else {
            id
        };

        Contact {
            id: final_id,
            location,
            department,
            position,
            full_name,
            internal_phone,
            mobile_phone,
            email,
            floor,
            office,
            is_hidden,
            is_personal_mobile: false,
        }
    }

    /// Преобразование контакта в мапу полей
    pub fn contact_to_map(contact: &Contact) -> HashMap<ContactField, String> {
        let mut map = HashMap::new();
        for field in ContactField::ALL {
            map.insert(field, field.get_value(contact).to_string());
        }
        map
    }
}
