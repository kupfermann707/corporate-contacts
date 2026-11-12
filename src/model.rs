use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    pub id: String,
    pub location: String,
    pub department: String,
    pub position: String,
    pub full_name: String,
    pub internal_phone: String,
    pub mobile_phone: String,
    pub email: String,
    pub floor: String,
    #[serde(alias = "room")]
    pub office: String,
    #[serde(default)]
    pub is_hidden: bool,
    #[serde(default)]
    pub is_personal_mobile: bool,
}

impl Contact {
    pub fn new(
        location: String,
        department: String,
        position: String,
        full_name: String,
        internal_phone: String,
        mobile_phone: String,
        email: String,
        floor: String,
        office: String,
    ) -> Self {
        let id = format!(
            "{}_{}_{}_{}",
            location,
            department,
            full_name,
            glib::uuid_string_random()
        );
        Self {
            id,
            location,
            department,
            position,
            full_name,
            internal_phone,
            mobile_phone,
            email,
            floor,
            office,
            is_hidden: false,
            is_personal_mobile: false,
        }
    }

    /// Short ID suitable for display in tickets / helpdesk requests
    #[allow(dead_code)]
    pub fn short_id(&self) -> &str {
        if let Some((_, rest)) = self.id.rsplit_once('_') {
            if rest.len() >= 8 {
                return &rest[..8];
            }
            return rest;
        }
        if self.id.len() > 8 {
            &self.id[..8]
        } else {
            &self.id
        }
    }

    /// Backwards compatibility accessor for room
    #[allow(dead_code)]
    pub fn room(&self) -> &str {
        &self.office
    }

    /// Extract initials from full name, e.g. "Иванов Иван" -> "ИИ"
    #[allow(dead_code)]
    pub fn initials(&self) -> String {
        let parts: Vec<&str> = self.full_name.split_whitespace().collect();
        if parts.is_empty() {
            return "?".to_string();
        }
        if parts.len() == 1 {
            return parts[0].chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".to_string());
        }
        let first = parts[0].chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
        let second = parts[1].chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
        format!("{}{}", first, second)
    }


    /// Check if contact matches search query, location filter, and department filter
    pub fn matches(&self, query: &str, location_filter: &str, department_filter: &str) -> bool {
        // Location filter
        if !location_filter.is_empty() && location_filter != "Все" && location_filter != "Все локации" {
            if self.location.trim().to_lowercase() != location_filter.trim().to_lowercase() {
                return false;
            }
        }

        // Department filter
        if !department_filter.is_empty() && department_filter != "Все отделы" {
            if self.department.trim().to_lowercase() != department_filter.trim().to_lowercase() {
                return false;
            }
        }

        // Search query
        if query.trim().is_empty() {
            return true;
        }

        let q = query.trim().to_lowercase();
        self.full_name.to_lowercase().contains(&q)
            || self.position.to_lowercase().contains(&q)
            || self.department.to_lowercase().contains(&q)
            || self.internal_phone.to_lowercase().contains(&q)
            || self.mobile_phone.to_lowercase().contains(&q)
            || self.email.to_lowercase().contains(&q)
            || self.floor.to_lowercase().contains(&q)
            || self.office.to_lowercase().contains(&q)
            || self.location.to_lowercase().contains(&q)
    }
}
