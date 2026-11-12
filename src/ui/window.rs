use crate::db::{Database, DbConfig};
use crate::model::Contact;
use crate::server::WebServer;
use crate::settings::AppSettings;
use crate::storage::StorageManager;
use crate::ui::auth_dialog::prompt_security_auth;
use crate::ui::contact_details::ContactDetailsView;
use crate::ui::contact_dialog::ContactDialog;
use crate::ui::import_dialog::ImportCsvDialog;
use crate::ui::settings_dialog::SettingsDialog;
use adw::prelude::*;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::{Arc, RwLock};

pub struct MainWindow {
    window: adw::ApplicationWindow,
}

impl MainWindow {
    pub fn new(app: &adw::Application) -> Self {
        let storage = Rc::new(StorageManager::new());
        let mut contacts_data = match storage.load_contacts() {
            Ok(c) => c,
            Err(err) => {
                eprintln!("Ошибка загрузки контактов: {}", err);
                Vec::new()
            }
        };

        let settings = Rc::new(RefCell::new(AppSettings::load()));
        let db_conn: Rc<RefCell<Option<Database>>> = Rc::new(RefCell::new(None));

        // Инициализация базы данных PostgreSQL (если настроена) и выполнение DDL
        if let Some(cfg) = DbConfig::from_env_or_settings(&settings.borrow()) {
            match Database::connect(cfg) {
                Ok(db) => {
                    if let Err(e) = db.init_tables() {
                        eprintln!("Ошибка авто-инициализации базы (DDL): {}", e);
                    } else {
                        match db.fetch_all() {
                            Ok(loaded) => {
                                if !loaded.is_empty() {
                                    contacts_data = loaded;
                                    let _ = storage.save_contacts(&contacts_data);
                                }
                            }
                            Err(e) => {
                                eprintln!("Ошибка загрузки контактов из PostgreSQL: {}", e);
                            }
                        }
                    }
                    *db_conn.borrow_mut() = Some(db);
                }
                Err(e) => {
                    eprintln!("PostgreSQL недоступен при старте ({}), используется локальный кэш", e);
                }
            }
        }

        let contacts: Rc<RefCell<Vec<Contact>>> = Rc::new(RefCell::new(contacts_data.clone()));
        let shared_contacts: Arc<RwLock<Vec<Contact>>> = Arc::new(RwLock::new(contacts_data));
        let selected_contact_id: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let filtered_contacts: Rc<RefCell<Vec<Contact>>> = Rc::new(RefCell::new(Vec::new()));
        let web_server: Rc<RefCell<Option<WebServer>>> = Rc::new(RefCell::new(None));

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Справочник контактов")
            .default_width(880)
            .default_height(650)
            .build();

        let toast_overlay = adw::ToastOverlay::new();

        // Helper to show toasts
        let toast_overlay_weak = toast_overlay.downgrade();
        let show_toast = move |msg: &str| {
            if let Some(overlay) = toast_overlay_weak.upgrade() {
                let toast = adw::Toast::new(msg);
                overlay.add_toast(toast);
            }
        };

        // Navigation Split View
        let split_view = adw::NavigationSplitView::builder()
            .min_sidebar_width(260.0)
            .max_sidebar_width(380.0)
            .sidebar_width_fraction(0.30)
            .build();

        // ----------------- SIDEBAR PAGE -----------------
        let sidebar_toolbar = adw::ToolbarView::new();
        let sidebar_header = adw::HeaderBar::new();

        // Add Contact button (+)
        let btn_add = gtk::Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("Добавить контакт")
            .css_classes(["suggested-action"])
            .build();
        sidebar_header.pack_start(&btn_add);

        // Sidebar Title
        let org_subtitle = settings.borrow().organization_name.clone();
        let sidebar_title = adw::WindowTitle::new("Контакты", &org_subtitle);
        sidebar_header.set_title_widget(Some(&sidebar_title));

        // Primary Menu Button
        let menu_model = gio::Menu::new();
        menu_model.append(Some("Настройки"), Some("app.settings"));
        menu_model.append(Some("Обновить базу"), Some("app.reload"));
        menu_model.append(Some("Импорт из CSV..."), Some("app.import_csv"));
        menu_model.append(Some("Экспорт в CSV..."), Some("app.export_csv"));
        menu_model.append(Some("Очистить базу..."), Some("app.clear_db"));
        menu_model.append(Some("О программе"), Some("app.about"));

        let menu_button = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu_model)
            .tooltip_text("Меню")
            .build();

        sidebar_header.pack_end(&menu_button);

        sidebar_toolbar.add_top_bar(&sidebar_header);

        // Search & Filter Box
        let sidebar_content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .margin_start(10)
            .margin_end(10)
            .margin_top(8)
            .margin_bottom(8)
            .build();

        // Search entry
        let search_entry = gtk::SearchEntry::builder()
            .placeholder_text("Поиск по имени, номеру, отделу...")
            .build();
        sidebar_content.append(&search_entry);

        // Filters row: Location & Department
        let filters_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .build();

        let location_items = gtk::StringList::new(&["Все локации"]);
        let location_dropdown = gtk::DropDown::builder()
            .model(&location_items)
            .hexpand(true)
            .tooltip_text("Фильтр по локации")
            .build();
        filters_box.append(&location_dropdown);

        let dept_items = gtk::StringList::new(&["Все отделы"]);
        let dept_dropdown = gtk::DropDown::builder()
            .model(&dept_items)
            .hexpand(true)
            .tooltip_text("Фильтр по отделу")
            .build();
        filters_box.append(&dept_dropdown);

        sidebar_content.append(&filters_box);

        // Visibility filter dropdown: "Все", "Только активные", "Скрытые / На увольнение"
        let status_items = gtk::StringList::new(&[
            "Все",
            "Только активные",
            "Скрытые / На увольнение",
        ]);
        let status_dropdown = gtk::DropDown::builder()
            .model(&status_items)
            .selected(0)
            .hexpand(true)
            .tooltip_text("Фильтр по статусу (Все / Только активные / Скрытые / На увольнение)")
            .build();

        sidebar_content.append(&status_dropdown);

        // Contacts ListBox
        let list_box = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(["navigation-sidebar"])
            .build();

        let list_scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&list_box)
            .hexpand(true)
            .vexpand(true)
            .build();
        sidebar_content.append(&list_scrolled);

        // Footer counter
        let count_label = gtk::Label::builder()
            .css_classes(["caption", "dim-label"])
            .halign(gtk::Align::Center)
            .margin_top(4)
            .margin_bottom(4)
            .build();
        sidebar_content.append(&count_label);

        sidebar_toolbar.set_content(Some(&sidebar_content));
        let sidebar_page = adw::NavigationPage::builder()
            .title("Контакты")
            .tag("sidebar")
            .child(&sidebar_toolbar)
            .build();

        // ----------------- CONTENT PAGE -----------------
        let content_toolbar = adw::ToolbarView::new();
        let content_header = adw::HeaderBar::new();

        // Action buttons: Edit & Delete
        let btn_edit = gtk::Button::builder()
            .icon_name("document-edit-symbolic")
            .tooltip_text("Редактировать контакт")
            .sensitive(false)
            .build();
        content_header.pack_end(&btn_edit);

        let btn_delete = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text("Удалить контакт")
            .css_classes(["destructive-action"])
            .sensitive(false)
            .build();
        content_header.pack_end(&btn_delete);

        content_toolbar.add_top_bar(&content_header);

        let show_toast_cloned = show_toast.clone();
        let details_view = Rc::new(ContactDetailsView::new(move |msg| {
            show_toast_cloned(msg);
        }));

        content_toolbar.set_content(Some(details_view.widget()));
        let content_page = adw::NavigationPage::builder()
            .title("Контакт")
            .tag("content")
            .child(&content_toolbar)
            .build();

        split_view.set_sidebar(Some(&sidebar_page));
        split_view.set_content(Some(&content_page));

        toast_overlay.set_child(Some(&split_view));
        window.set_content(Some(&toast_overlay));

        // ----------------- WEB SERVER CONTROLLER -----------------
        let active_url: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

        let toggle_server = {
            let web_server = web_server.clone();
            let shared_contacts = shared_contacts.clone();
            let active_url = active_url.clone();
            let show_toast = show_toast.clone();
            let settings = settings.clone();

            Rc::new(move |enable: bool, port: u16| {
                if enable {
                    // Stop existing if any
                    if let Some(mut old) = web_server.borrow_mut().take() {
                        old.stop();
                    }

                    let settings_clone = settings.borrow().clone();
                    let settings_for_toggle = settings_clone.clone();
                    let shared_for_cb = shared_contacts.clone();
                    let toggle_cb: Option<crate::server::ToggleVisibilityCallback> = Some(std::sync::Arc::new(
                        move |id: &str, is_hidden: bool| -> Result<(), String> {
                            if let Some(cfg) = DbConfig::from_env_or_settings(&settings_for_toggle) {
                                if let Ok(db) = Database::connect(cfg) {
                                    let _ = db.set_hidden(id, is_hidden);
                                }
                            }
                            if let Ok(guard) = shared_for_cb.read() {
                                let sm = StorageManager::new();
                                let _ = sm.save_contacts(&guard);
                            }
                            Ok(())
                        },
                    ));

                    let is_https = settings_clone.https_enabled;
                    let cert_path = if !settings_clone.tls_cert_path.trim().is_empty() {
                        Some(std::path::PathBuf::from(&settings_clone.tls_cert_path))
                    } else {
                        None
                    };
                    let key_path = if !settings_clone.tls_key_path.trim().is_empty() {
                        Some(std::path::PathBuf::from(&settings_clone.tls_key_path))
                    } else {
                        None
                    };

                    match WebServer::start_configured(port, shared_contacts.clone(), toggle_cb, is_https, cert_path, key_path) {
                        Ok(srv) => {
                            let url = srv.url();
                            *active_url.borrow_mut() = Some(url.clone());
                            *web_server.borrow_mut() = Some(srv);

                            let mut s = settings.borrow_mut();
                            s.web_server_enabled = true;
                            s.web_server_port = port;
                            let _ = s.save();

                            show_toast(&format!(
                                "Веб-сервер запущен ({}): {}",
                                if is_https { "HTTPS / 152-ФЗ" } else { "HTTP" },
                                url
                            ));
                        }
                        Err(err) => {
                            *active_url.borrow_mut() = None;
                            show_toast(&format!("Ошибка запуска сервера: {}", err));
                        }
                    }
                } else {
                    if let Some(mut srv) = web_server.borrow_mut().take() {
                        srv.stop();
                    }
                    *active_url.borrow_mut() = None;

                    let mut s = settings.borrow_mut();
                    s.web_server_enabled = false;
                    let _ = s.save();

                    show_toast("Веб-сервер в локальной сети остановлен");
                }
            })
        };

        // ----------------- DATA LOGIC & UI UPDATES -----------------

        let populate_locations = {
            let location_items = location_items.clone();
            let location_dropdown = location_dropdown.clone();
            Rc::new(move |locations: &[String]| {
                let current_selected_text = location_dropdown
                    .selected_item()
                    .and_then(|item| item.downcast::<gtk::StringObject>().ok())
                    .map(|so| so.string().to_string())
                    .unwrap_or_else(|| "Все локации".to_string());

                while location_items.n_items() > 0 {
                    location_items.remove(0);
                }
                location_items.append("Все локации");
                let mut found_idx = 0;
                for (idx, l) in locations.iter().enumerate() {
                    location_items.append(l);
                    if l == &current_selected_text {
                        found_idx = (idx + 1) as u32;
                    }
                }
                location_dropdown.set_selected(found_idx);
            })
        };

        let populate_depts = {
            let dept_items = dept_items.clone();
            let dept_dropdown = dept_dropdown.clone();
            Rc::new(move |departments: &[String]| {
                let current_selected_text = dept_dropdown
                    .selected_item()
                    .and_then(|item| item.downcast::<gtk::StringObject>().ok())
                    .map(|so| so.string().to_string())
                    .unwrap_or_else(|| "Все отделы".to_string());

                while dept_items.n_items() > 0 {
                    dept_items.remove(0);
                }
                dept_items.append("Все отделы");
                let mut found_idx = 0;
                for (idx, d) in departments.iter().enumerate() {
                    dept_items.append(d);
                    if d == &current_selected_text {
                        found_idx = (idx + 1) as u32;
                    }
                }
                dept_dropdown.set_selected(found_idx);
            })
        };

        populate_locations(&extract_locations(&contacts.borrow()));
        populate_depts(&extract_departments(&contacts.borrow()));

        let refresh_list = {
            let contacts = contacts.clone();
            let filtered_contacts = filtered_contacts.clone();
            let search_entry = search_entry.clone();
            let location_dropdown = location_dropdown.clone();
            let dept_dropdown = dept_dropdown.clone();
            let status_dropdown = status_dropdown.clone();
            let list_box = list_box.clone();
            let count_label = count_label.clone();
            let selected_contact_id = selected_contact_id.clone();
            let details_view = details_view.clone();
            let btn_edit = btn_edit.clone();
            let btn_delete = btn_delete.clone();

            Rc::new(move || {
                let query = search_entry.text().to_string();
                let loc_str = location_dropdown
                    .selected_item()
                    .and_then(|item| item.downcast::<gtk::StringObject>().ok())
                    .map(|so| so.string().to_string())
                    .unwrap_or_else(|| "Все локации".to_string());

                let dept_str = dept_dropdown
                    .selected_item()
                    .and_then(|item| item.downcast::<gtk::StringObject>().ok())
                    .map(|so| so.string().to_string())
                    .unwrap_or_else(|| "Все отделы".to_string());

                let status_idx = status_dropdown.selected();

                let all_contacts = contacts.borrow();
                let matching: Vec<Contact> = all_contacts
                    .iter()
                    .filter(|c| {
                        match status_idx {
                            1 => {
                                if c.is_hidden {
                                    return false;
                                }
                            }
                            2 => {
                                if !c.is_hidden {
                                    return false;
                                }
                            }
                            _ => {} // 0: Все
                        }
                        c.matches(&query, &loc_str, &dept_str)
                    })
                    .cloned()
                    .collect();

                let total_count = all_contacts.len();
                let matched_count = matching.len();

                count_label.set_text(&format!("Показано: {} из {}", matched_count, total_count));

                while let Some(child) = list_box.first_child() {
                    list_box.remove(&child);
                }

                *filtered_contacts.borrow_mut() = matching.clone();

                let mut row_to_select: Option<gtk::ListBoxRow> = None;
                let current_sel_id = selected_contact_id.borrow().clone();

                for contact in &matching {
                    let row = adw::ActionRow::builder()
                        .subtitle(&format!(
                            "{} · {}",
                            if contact.position.is_empty() { "—" } else { &contact.position },
                            contact.department
                        ))
                        .subtitle_lines(1)
                        .activatable(true)
                        .selectable(true)
                        .build();

                    if contact.is_hidden {
                        row.set_use_markup(true);
                        row.set_title(&format!(
                            "<span alpha='65%'><s>{}</s></span>",
                            glib::markup_escape_text(&contact.full_name)
                        ));
                        row.add_css_class("dim-label");
                    } else {
                        row.set_use_markup(false);
                        row.set_title(&contact.full_name);
                    }

                    let avatar = adw::Avatar::builder()
                        .size(36)
                        .text(&contact.full_name)
                        .show_initials(true)
                        .build();
                    row.add_prefix(&avatar);

                    if contact.is_hidden {
                        let hidden_badge = gtk::Label::builder()
                            .label("Скрыт")
                            .css_classes(["caption", "badge", "error"])
                            .valign(gtk::Align::Center)
                            .tooltip_text("Контакт скрыт из телефонной книги")
                            .build();
                        row.add_suffix(&hidden_badge);
                    }

                    if !contact.location.trim().is_empty() {
                        let badge = gtk::Label::builder()
                            .label(&contact.location)
                            .css_classes(["caption", "badge-location"])
                            .valign(gtk::Align::Center)
                            .build();
                        row.add_suffix(&badge);
                    }

                    let list_box_row = gtk::ListBoxRow::builder()
                        .child(&row)
                        .build();

                    list_box_row.set_widget_name(&contact.id);

                    if let Some(ref id) = current_sel_id {
                        if id == &contact.id {
                            row_to_select = Some(list_box_row.clone());
                        }
                    }

                    list_box.append(&list_box_row);
                }

                if let Some(ref sel_row) = row_to_select {
                    list_box.select_row(Some(sel_row));
                } else if current_sel_id.is_some() {
                    *selected_contact_id.borrow_mut() = None;
                    details_view.show_empty();
                    btn_edit.set_sensitive(false);
                    btn_delete.set_sensitive(false);
                }
            })
        };

        refresh_list();

        // Connect filter changes
        {
            let refresh = refresh_list.clone();
            search_entry.connect_search_changed(move |_| {
                refresh();
            });
        }
        {
            let refresh = refresh_list.clone();
            location_dropdown.connect_selected_notify(move |_| {
                refresh();
            });
        }
        {
            let refresh = refresh_list.clone();
            dept_dropdown.connect_selected_notify(move |_| {
                refresh();
            });
        }
        {
            let refresh = refresh_list.clone();
            status_dropdown.connect_selected_notify(move |_| {
                refresh();
            });
        }

        // Connect row selection
        {
            let contacts = contacts.clone();
            let selected_contact_id = selected_contact_id.clone();
            let details_view = details_view.clone();
            let split_view = split_view.clone();
            let btn_edit = btn_edit.clone();
            let btn_delete = btn_delete.clone();

            list_box.connect_row_selected(move |_, row_opt| {
                if let Some(row) = row_opt {
                    let contact_id = row.widget_name().to_string();
                    let all = contacts.borrow();
                    if let Some(contact) = all.iter().find(|c| c.id == contact_id) {
                        *selected_contact_id.borrow_mut() = Some(contact.id.clone());
                        details_view.update(Some(contact));
                        btn_edit.set_sensitive(true);
                        btn_delete.set_sensitive(true);
                        if contact.is_hidden {
                            btn_delete.set_icon_name("edit-undo-symbolic");
                            btn_delete.set_tooltip_text(Some("Восстановить или удалить контакт"));
                            btn_delete.remove_css_class("destructive-action");
                            btn_delete.add_css_class("suggested-action");
                        } else {
                            btn_delete.set_icon_name("user-trash-symbolic");
                            btn_delete.set_tooltip_text(Some("Скрыть или удалить контакт"));
                            btn_delete.remove_css_class("suggested-action");
                            btn_delete.add_css_class("destructive-action");
                        }
                        split_view.set_show_content(true);
                    }
                } else {
                    *selected_contact_id.borrow_mut() = None;
                    details_view.show_empty();
                    btn_edit.set_sensitive(false);
                    btn_delete.set_sensitive(false);
                    btn_delete.set_icon_name("user-trash-symbolic");
                    btn_delete.set_tooltip_text(Some("Скрыть контакт"));
                    btn_delete.remove_css_class("suggested-action");
                    btn_delete.add_css_class("destructive-action");
                }
            });
        }

        // Connect Add button (+)
        {
            let window_widget = window.clone();
            let contacts = contacts.clone();
            let shared_contacts = shared_contacts.clone();
            let storage = storage.clone();
            let refresh_list = refresh_list.clone();
            let populate_locations = populate_locations.clone();
            let populate_depts = populate_depts.clone();
            let selected_contact_id = selected_contact_id.clone();
            let show_toast = show_toast.clone();
            let db_conn = db_conn.clone();
            let settings = settings.clone();

            btn_add.connect_clicked(move |_| {
                let depts = extract_departments(&contacts.borrow());
                let contacts = contacts.clone();
                let shared_contacts = shared_contacts.clone();
                let storage = storage.clone();
                let refresh_list = refresh_list.clone();
                let populate_locations = populate_locations.clone();
                let populate_depts = populate_depts.clone();
                let selected_contact_id = selected_contact_id.clone();
                let show_toast = show_toast.clone();
                let db_conn = db_conn.clone();
                let settings = settings.clone();

                ContactDialog::show(&window_widget, None, &depts, move |mut new_contact| {
                    if settings.borrow().is_db_configured() {
                        let insert_res = match get_or_connect_db(&db_conn, &settings.borrow()) {
                            Ok(db) => match db.insert_contact(&new_contact) {
                                Ok(db_id) => Ok(db_id),
                                Err(err) => {
                                    *db_conn.borrow_mut() = None;
                                    if let Ok(reconnected_db) = get_or_connect_db(&db_conn, &settings.borrow()) {
                                        reconnected_db.insert_contact(&new_contact)
                                    } else {
                                        Err(err)
                                    }
                                }
                            },
                            Err(e) => Err(format!("Не удалось подключиться: {}", e)),
                        };

                        match insert_res {
                            Ok(db_id) => {
                                new_contact.id = db_id;
                            }
                            Err(e) => {
                                show_toast(&format!("Внимание: Не удалось сохранить в PostgreSQL: {}. Запись сохранена только локально.", e));
                            }
                        }
                    }

                    let new_id = new_contact.id.clone();
                    let name = new_contact.full_name.clone();
                    {
                        let mut all = contacts.borrow_mut();
                        all.push(new_contact);
                        all.sort_by(|a, b| a.full_name.to_lowercase().cmp(&b.full_name.to_lowercase()));
                        *shared_contacts.write().unwrap() = all.clone();
                        if let Err(err) = storage.save_contacts(&all) {
                            show_toast(&format!("Ошибка сохранения локальной копии: {}", err));
                        }
                    }
                    populate_locations(&extract_locations(&contacts.borrow()));
                    populate_depts(&extract_departments(&contacts.borrow()));
                    *selected_contact_id.borrow_mut() = Some(new_id.clone());
                    refresh_list();
                    crate::audit::AuditLogger::log(
                        "CONTACT_CREATE",
                        "local_user",
                        &format!("Создан новый контакт id={}, имя={}", new_id, name),
                    );
                    show_toast(&format!("Контакт «{}» добавлен", name));
                });
            });
        }

        // Connect Edit button
        {
            let window_widget = window.clone();
            let contacts = contacts.clone();
            let shared_contacts = shared_contacts.clone();
            let storage = storage.clone();
            let refresh_list = refresh_list.clone();
            let populate_locations = populate_locations.clone();
            let populate_depts = populate_depts.clone();
            let selected_contact_id = selected_contact_id.clone();
            let details_view = details_view.clone();
            let show_toast = show_toast.clone();
            let db_conn = db_conn.clone();
            let settings = settings.clone();

            btn_edit.connect_clicked(move |_| {
                let cur_id_opt = selected_contact_id.borrow().clone();
                if let Some(cur_id) = cur_id_opt {
                    let contact_opt = {
                        let all = contacts.borrow();
                        all.iter().find(|c| c.id == cur_id).cloned()
                    };

                    if let Some(contact) = contact_opt {
                        let depts = extract_departments(&contacts.borrow());
                        let contacts = contacts.clone();
                        let shared_contacts = shared_contacts.clone();
                        let storage = storage.clone();
                        let refresh_list = refresh_list.clone();
                        let populate_locations = populate_locations.clone();
                        let populate_depts = populate_depts.clone();
                        let details_view = details_view.clone();
                        let show_toast = show_toast.clone();
                        let db_conn = db_conn.clone();
                        let settings = settings.clone();

                        ContactDialog::show(&window_widget, Some(&contact), &depts, move |updated_contact| {
                            if settings.borrow().is_db_configured() {
                                let update_res = match get_or_connect_db(&db_conn, &settings.borrow()) {
                                    Ok(db) => match db.update_contact(&updated_contact) {
                                        Ok(()) => Ok(()),
                                        Err(err) => {
                                            *db_conn.borrow_mut() = None;
                                            if let Ok(reconnected_db) = get_or_connect_db(&db_conn, &settings.borrow()) {
                                                reconnected_db.update_contact(&updated_contact)
                                            } else {
                                                Err(err)
                                            }
                                        }
                                    },
                                    Err(e) => Err(format!("Не удалось подключиться: {}", e)),
                                };

                                if let Err(e) = update_res {
                                    show_toast(&format!("Внимание: Не удалось обновить запись в PostgreSQL: {}", e));
                                }
                            }

                            let name = updated_contact.full_name.clone();
                            {
                                let mut all = contacts.borrow_mut();
                                if let Some(idx) = all.iter().position(|c| c.id == updated_contact.id) {
                                    all[idx] = updated_contact.clone();
                                    all.sort_by(|a, b| a.full_name.to_lowercase().cmp(&b.full_name.to_lowercase()));
                                }
                                *shared_contacts.write().unwrap() = all.clone();
                                if let Err(err) = storage.save_contacts(&all) {
                                    show_toast(&format!("Ошибка сохранения локальной копии: {}", err));
                                }
                            }
                            populate_locations(&extract_locations(&contacts.borrow()));
                            populate_depts(&extract_departments(&contacts.borrow()));
                            details_view.update(Some(&updated_contact));
                            refresh_list();
                            crate::audit::AuditLogger::log(
                                "CONTACT_UPDATE",
                                "local_user",
                                &format!("Обновлен контакт id={}, имя={}", updated_contact.id, name),
                            );
                            show_toast(&format!("Контакт «{}» обновлён", name));
                        });
                    }
                }
            });
        }

        // Connect Delete / Hide / Restore button
        {
            let window_widget = window.clone();
            let contacts = contacts.clone();
            let shared_contacts = shared_contacts.clone();
            let storage = storage.clone();
            let refresh_list = refresh_list.clone();
            let populate_locations = populate_locations.clone();
            let populate_depts = populate_depts.clone();
            let selected_contact_id = selected_contact_id.clone();
            let details_view = details_view.clone();
            let show_toast = show_toast.clone();
            let db_conn = db_conn.clone();
            let settings = settings.clone();
            let btn_delete_btn = btn_delete.clone();
            let btn_edit_btn = btn_edit.clone();

            btn_delete.connect_clicked(move |_| {
                let cur_id_opt = selected_contact_id.borrow().clone();
                if let Some(cur_id) = cur_id_opt {
                    let contact_opt = {
                        let all = contacts.borrow();
                        all.iter().find(|c| c.id == cur_id).cloned()
                    };

                    if let Some(contact) = contact_opt {
                        let name = contact.full_name.clone();
                        let cur_id_str = cur_id.clone();

                        if contact.is_hidden {
                            // Скрытый контакт: диалог с выбором [Отмена], [Восстановить] (без пароля) и [Удалить] (с паролем)
                            let dialog = adw::AlertDialog::builder()
                                .heading("Восстановить или удалить контакт?")
                                .body(&format!(
                                    "Контакт «{}» (ID: {}) сейчас скрыт из телефонной книги.\n\n• Восстановить — контакт снова станет видимым для всех пользователей.\n• Удалить — запись будет безвозвратно удалена из базы данных.",
                                    name, cur_id_str
                                ))
                                .build();

                            dialog.add_response("cancel", "Отмена");
                            dialog.add_response("restore", "Восстановить");
                            dialog.add_response("delete", "Удалить");
                            dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
                            dialog.set_response_appearance("restore", adw::ResponseAppearance::Suggested);
                            dialog.set_default_response(Some("cancel"));
                            dialog.set_close_response("cancel");

                            let contacts = contacts.clone();
                            let shared_contacts = shared_contacts.clone();
                            let storage = storage.clone();
                            let refresh_list = refresh_list.clone();
                            let populate_locations = populate_locations.clone();
                            let populate_depts = populate_depts.clone();
                            let details_view = details_view.clone();
                            let show_toast = show_toast.clone();
                            let db_conn = db_conn.clone();
                            let settings = settings.clone();
                            let btn_delete_btn = btn_delete_btn.clone();
                            let btn_edit_btn = btn_edit_btn.clone();
                            let selected_contact_id = selected_contact_id.clone();
                            let cur_id_dialog = cur_id_str.clone();
                            let name_dialog = name.clone();
                            let contact_for_del = contact.clone();
                            let dialog_win = window_widget.clone();

                            dialog.choose(&window_widget, None::<&gio::Cancellable>, move |response| {
                                match response.as_str() {
                                    "restore" => {
                                        // Восстановление контакта БЕЗ запроса пароля
                                        if settings.borrow().is_db_configured() {
                                            let restore_res = match get_or_connect_db(&db_conn, &settings.borrow()) {
                                                Ok(db) => match db.set_hidden(&cur_id_dialog, false) {
                                                    Ok(()) => Ok(()),
                                                    Err(err) => {
                                                        *db_conn.borrow_mut() = None;
                                                        if let Ok(reconnected_db) = get_or_connect_db(&db_conn, &settings.borrow()) {
                                                            reconnected_db.set_hidden(&cur_id_dialog, false)
                                                        } else {
                                                            Err(err)
                                                        }
                                                    }
                                                },
                                                Err(e) => Err(format!("Не удалось подключиться: {}", e)),
                                            };

                                            if let Err(e) = restore_res {
                                                show_toast(&format!("Внимание: Не удалось восстановить запись в PostgreSQL: {}", e));
                                            }
                                        }

                                        {
                                            let mut all = contacts.borrow_mut();
                                            if let Some(c) = all.iter_mut().find(|c| c.id == cur_id_dialog) {
                                                c.is_hidden = false;
                                            }
                                            *shared_contacts.write().unwrap() = all.clone();
                                            if let Err(err) = storage.save_contacts(&all) {
                                                show_toast(&format!("Ошибка сохранения локальной копии: {}", err));
                                            }
                                        }

                                        if let Some(updated) = contacts.borrow().iter().find(|c| c.id == cur_id_dialog).cloned() {
                                            details_view.update(Some(&updated));
                                        }

                                        btn_delete_btn.set_icon_name("user-trash-symbolic");
                                        btn_delete_btn.set_tooltip_text(Some("Скрыть или удалить контакт"));
                                        btn_delete_btn.remove_css_class("suggested-action");
                                        btn_delete_btn.add_css_class("destructive-action");

                                        populate_locations(&extract_locations(&contacts.borrow()));
                                        populate_depts(&extract_departments(&contacts.borrow()));
                                        refresh_list();
                                        crate::audit::AuditLogger::log(
                                            "CONTACT_RESTORE",
                                            "local_user",
                                            &format!("Восстановлена видимость контакта id={}, имя={}", cur_id_dialog, name_dialog),
                                        );
                                        show_toast(&format!("Контакт «{}» восстановлен в справочнике", name_dialog));
                                    }
                                    "delete" => {
                                        // Полное удаление контакта — С ЗАПРОСОМ пароля/TOTP
                                        let auth_win = dialog_win.clone();
                                        let auth_settings = settings.borrow().clone();

                                        let contacts = contacts.clone();
                                        let shared_contacts = shared_contacts.clone();
                                        let storage = storage.clone();
                                        let refresh_list = refresh_list.clone();
                                        let populate_locations = populate_locations.clone();
                                        let populate_depts = populate_depts.clone();
                                        let details_view = details_view.clone();
                                        let show_toast = show_toast.clone();
                                        let db_conn = db_conn.clone();
                                        let settings = settings.clone();
                                        let btn_delete_btn = btn_delete_btn.clone();
                                        let btn_edit_btn = btn_edit_btn.clone();
                                        let selected_contact_id = selected_contact_id.clone();
                                        let cur_id_dialog = cur_id_dialog.clone();
                                        let name_dialog = name_dialog.clone();
                                        let contact_for_del = contact_for_del.clone();

                                        prompt_security_auth(
                                            &auth_win,
                                            &auth_settings,
                                            "Удаление контакта",
                                            &format!("Контакт «{}» будет безвозвратно удалён из базы данных.", name_dialog),
                                            true,
                                            move || {
                                                if settings.borrow().is_db_configured() {
                                                    let del_res = match get_or_connect_db(&db_conn, &settings.borrow()) {
                                                        Ok(db) => match db.delete_contact(&contact_for_del) {
                                                            Ok(()) => Ok(()),
                                                            Err(err) => {
                                                                *db_conn.borrow_mut() = None;
                                                                if let Ok(reconnected_db) = get_or_connect_db(&db_conn, &settings.borrow()) {
                                                                    reconnected_db.delete_contact(&contact_for_del)
                                                                } else {
                                                                    Err(err)
                                                                }
                                                            }
                                                        },
                                                        Err(e) => Err(format!("Не удалось подключиться: {}", e)),
                                                    };

                                                    if let Err(e) = del_res {
                                                        show_toast(&format!("Внимание: Не удалось удалить запись из PostgreSQL: {}", e));
                                                    }
                                                }

                                                {
                                                    let mut all = contacts.borrow_mut();
                                                    all.retain(|c| c.id != cur_id_dialog);
                                                    *shared_contacts.write().unwrap() = all.clone();
                                                    if let Err(err) = storage.save_contacts(&all) {
                                                        show_toast(&format!("Ошибка сохранения локальной копии: {}", err));
                                                    }
                                                }

                                                *selected_contact_id.borrow_mut() = None;
                                                details_view.show_empty();
                                                btn_edit_btn.set_sensitive(false);
                                                btn_delete_btn.set_sensitive(false);
                                                btn_delete_btn.set_icon_name("user-trash-symbolic");
                                                btn_delete_btn.set_tooltip_text(Some("Скрыть или удалить контакт"));
                                                btn_delete_btn.remove_css_class("suggested-action");
                                                btn_delete_btn.add_css_class("destructive-action");

                                                populate_locations(&extract_locations(&contacts.borrow()));
                                                populate_depts(&extract_departments(&contacts.borrow()));
                                                refresh_list();
                                                crate::audit::AuditLogger::log(
                                                    "CONTACT_DELETE",
                                                    "local_user",
                                                    &format!("Удален контакт из базы id={}, имя={}", cur_id_dialog, name_dialog),
                                                );
                                                show_toast(&format!("Контакт «{}» удалён из базы", name_dialog));
                                            },
                                        );
                                    }
                                    _ => {}
                                }
                            });
                        } else {
                            // Активный контакт: диалог с выбором [Отмена], [Скрыть] (без пароля) и [Удалить] (с паролем)
                            let dialog = adw::AlertDialog::builder()
                                .heading("Скрыть или удалить контакт?")
                                .body(&format!(
                                    "Контакт «{}» (ID: {}).\n\n• Скрыть — контакт перестанет отображаться в телефонной книге, но сохранится в списке скрытых.\n• Удалить — запись будет безвозвратно удалена из базы данных.",
                                    name, cur_id_str
                                ))
                                .build();

                            dialog.add_response("cancel", "Отмена");
                            dialog.add_response("hide", "Скрыть");
                            dialog.add_response("delete", "Удалить");
                            dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
                            dialog.set_response_appearance("hide", adw::ResponseAppearance::Suggested);
                            dialog.set_default_response(Some("cancel"));
                            dialog.set_close_response("cancel");

                            let contacts = contacts.clone();
                            let shared_contacts = shared_contacts.clone();
                            let storage = storage.clone();
                            let refresh_list = refresh_list.clone();
                            let populate_locations = populate_locations.clone();
                            let populate_depts = populate_depts.clone();
                            let details_view = details_view.clone();
                            let show_toast = show_toast.clone();
                            let db_conn = db_conn.clone();
                            let settings = settings.clone();
                            let btn_delete_btn = btn_delete_btn.clone();
                            let btn_edit_btn = btn_edit_btn.clone();
                            let selected_contact_id = selected_contact_id.clone();
                            let cur_id_dialog = cur_id_str.clone();
                            let name_dialog = name.clone();
                            let contact_for_del = contact.clone();
                            let dialog_win = window_widget.clone();

                            dialog.choose(&window_widget, None::<&gio::Cancellable>, move |response| {
                                match response.as_str() {
                                    "hide" => {
                                        // Скрытие контакта БЕЗ запроса пароля
                                        if settings.borrow().is_db_configured() {
                                            let hide_res = match get_or_connect_db(&db_conn, &settings.borrow()) {
                                                Ok(db) => match db.set_hidden(&cur_id_dialog, true) {
                                                    Ok(()) => Ok(()),
                                                    Err(err) => {
                                                        *db_conn.borrow_mut() = None;
                                                        if let Ok(reconnected_db) = get_or_connect_db(&db_conn, &settings.borrow()) {
                                                            reconnected_db.set_hidden(&cur_id_dialog, true)
                                                        } else {
                                                            Err(err)
                                                        }
                                                    }
                                                },
                                                Err(e) => Err(format!("Не удалось подключиться: {}", e)),
                                            };

                                            if let Err(e) = hide_res {
                                                show_toast(&format!("Внимание: Не удалось скрыть запись в PostgreSQL: {}", e));
                                            }
                                        }

                                        {
                                            let mut all = contacts.borrow_mut();
                                            if let Some(c) = all.iter_mut().find(|c| c.id == cur_id_dialog) {
                                                c.is_hidden = true;
                                            }
                                            *shared_contacts.write().unwrap() = all.clone();
                                            if let Err(err) = storage.save_contacts(&all) {
                                                show_toast(&format!("Ошибка сохранения локальной копии: {}", err));
                                            }
                                        }

                                        if let Some(updated) = contacts.borrow().iter().find(|c| c.id == cur_id_dialog).cloned() {
                                            details_view.update(Some(&updated));
                                        }

                                        btn_delete_btn.set_icon_name("edit-undo-symbolic");
                                        btn_delete_btn.set_tooltip_text(Some("Восстановить или удалить контакт"));
                                        btn_delete_btn.remove_css_class("destructive-action");
                                        btn_delete_btn.add_css_class("suggested-action");

                                        populate_locations(&extract_locations(&contacts.borrow()));
                                        populate_depts(&extract_departments(&contacts.borrow()));
                                        refresh_list();
                                        crate::audit::AuditLogger::log(
                                            "CONTACT_HIDE",
                                            "local_user",
                                            &format!("Скрыта карточка контакта (152-ФЗ) id={}, имя={}", cur_id_dialog, name_dialog),
                                        );
                                        show_toast(&format!("Контакт «{}» скрыт из справочника", name_dialog));
                                    }
                                    "delete" => {
                                        // Полное удаление контакта — С ЗАПРОСОМ пароля/TOTP
                                        let auth_win = dialog_win.clone();
                                        let auth_settings = settings.borrow().clone();

                                        let contacts = contacts.clone();
                                        let shared_contacts = shared_contacts.clone();
                                        let storage = storage.clone();
                                        let refresh_list = refresh_list.clone();
                                        let populate_locations = populate_locations.clone();
                                        let populate_depts = populate_depts.clone();
                                        let details_view = details_view.clone();
                                        let show_toast = show_toast.clone();
                                        let db_conn = db_conn.clone();
                                        let settings = settings.clone();
                                        let btn_delete_btn = btn_delete_btn.clone();
                                        let btn_edit_btn = btn_edit_btn.clone();
                                        let selected_contact_id = selected_contact_id.clone();
                                        let cur_id_dialog = cur_id_dialog.clone();
                                        let name_dialog = name_dialog.clone();
                                        let contact_for_del = contact_for_del.clone();

                                        prompt_security_auth(
                                            &auth_win,
                                            &auth_settings,
                                            "Удаление контакта",
                                            &format!("Контакт «{}» будет безвозвратно удалён из базы данных.", name_dialog),
                                            true,
                                            move || {
                                                if settings.borrow().is_db_configured() {
                                                    let del_res = match get_or_connect_db(&db_conn, &settings.borrow()) {
                                                        Ok(db) => match db.delete_contact(&contact_for_del) {
                                                            Ok(()) => Ok(()),
                                                            Err(err) => {
                                                                *db_conn.borrow_mut() = None;
                                                                if let Ok(reconnected_db) = get_or_connect_db(&db_conn, &settings.borrow()) {
                                                                    reconnected_db.delete_contact(&contact_for_del)
                                                                } else {
                                                                    Err(err)
                                                                }
                                                            }
                                                        },
                                                        Err(e) => Err(format!("Не удалось подключиться: {}", e)),
                                                    };

                                                    if let Err(e) = del_res {
                                                        show_toast(&format!("Внимание: Не удалось удалить запись из PostgreSQL: {}", e));
                                                    }
                                                }

                                                {
                                                    let mut all = contacts.borrow_mut();
                                                    all.retain(|c| c.id != cur_id_dialog);
                                                    *shared_contacts.write().unwrap() = all.clone();
                                                    if let Err(err) = storage.save_contacts(&all) {
                                                        show_toast(&format!("Ошибка сохранения локальной копии: {}", err));
                                                    }
                                                }

                                                *selected_contact_id.borrow_mut() = None;
                                                details_view.show_empty();
                                                btn_edit_btn.set_sensitive(false);
                                                btn_delete_btn.set_sensitive(false);
                                                btn_delete_btn.set_icon_name("user-trash-symbolic");
                                                btn_delete_btn.set_tooltip_text(Some("Скрыть или удалить контакт"));
                                                btn_delete_btn.remove_css_class("suggested-action");
                                                btn_delete_btn.add_css_class("destructive-action");

                                                populate_locations(&extract_locations(&contacts.borrow()));
                                                populate_depts(&extract_departments(&contacts.borrow()));
                                                refresh_list();
                                                crate::audit::AuditLogger::log(
                                                    "CONTACT_DELETE",
                                                    "local_user",
                                                    &format!("Удален контакт из базы id={}, имя={}", cur_id_dialog, name_dialog),
                                                );
                                                show_toast(&format!("Контакт «{}» удалён из базы", name_dialog));
                                            },
                                        );
                                    }
                                    _ => {}
                                }
                            });
                        }
                    }
                }
            });
        }

        // App Action: Reload (enabled only when SQL / PostgreSQL is configured)
        let action_reload = gio::SimpleAction::new("reload", None);
        action_reload.set_enabled(settings.borrow().is_db_configured());
        app.add_action(&action_reload);

        {
            let settings = settings.clone();
            let contacts = contacts.clone();
            let shared_contacts = shared_contacts.clone();
            let storage = storage.clone();
            let refresh_list = refresh_list.clone();
            let populate_locations = populate_locations.clone();
            let populate_depts = populate_depts.clone();
            let show_toast = show_toast.clone();
            let db_conn = db_conn.clone();

            action_reload.connect_activate(move |_, _| {
                let cfg_opt = DbConfig::from_env_or_settings(&settings.borrow());
                let (host, port) = match &cfg_opt {
                    Some(c) => (c.host.clone(), c.port),
                    None => {
                        show_toast("Настройки подключения к базе данных не заданы");
                        return;
                    }
                };

                show_toast(&format!("Подключение к PostgreSQL ({}:{})...", host, port));

                let contacts = contacts.clone();
                let shared_contacts = shared_contacts.clone();
                let storage = storage.clone();
                let refresh_list = refresh_list.clone();
                let populate_locations = populate_locations.clone();
                let populate_depts = populate_depts.clone();
                let show_toast = show_toast.clone();
                let cfg = cfg_opt.unwrap();
                match Database::connect(cfg) {
                    Ok(db) => {
                        if let Err(e) = db.init_tables() {
                            show_toast(&format!("Ошибка авто-инициализации DDL: {}", e));
                            return;
                        }
                        match db.fetch_all() {
                            Ok(loaded) => {
                                let count = loaded.len();
                                *db_conn.borrow_mut() = Some(db);
                                *shared_contacts.write().unwrap() = loaded.clone();
                                *contacts.borrow_mut() = loaded.clone();
                                let _ = storage.save_contacts(&loaded);
                                populate_locations(&extract_locations(&contacts.borrow()));
                                populate_depts(&extract_departments(&contacts.borrow()));
                                refresh_list();
                                show_toast(&format!(
                                    "✓ Подключение к PostgreSQL ({}:{}) успешно. Синхронизировано {} контактов",
                                    host, port, count
                                ));
                            }
                            Err(e) => {
                                show_toast(&format!(
                                    "Ошибка загрузки контактов из PostgreSQL ({}:{}): {}",
                                    host, port, e
                                ));
                            }
                        }
                    }
                    Err(err) => {
                        show_toast(&format!(
                            "Ошибка подключения к PostgreSQL ({}:{}): {}",
                            host, port, err
                        ));
                    }
                }
            });
        }

        // Open Settings logic
        let open_settings = {
            let window_widget = window.clone();
            let settings_clone = settings.clone();
            let toggle_server = toggle_server.clone();
            let active_url = active_url.clone();
            let web_server = web_server.clone();
            let sidebar_title = sidebar_title.clone();
            let action_reload_cloned = action_reload.clone();
            let db_conn = db_conn.clone();

            Rc::new(move |page: Option<&str>| {
                let is_running = web_server.borrow().as_ref().map(|s| s.is_running()).unwrap_or(false);
                let url = active_url.borrow().clone();
                let toggle_server = toggle_server.clone();
                let sidebar_title = sidebar_title.clone();
                let action_reload_cloned = action_reload_cloned.clone();
                let settings_for_cb = settings_clone.clone();
                let db_conn = db_conn.clone();

                SettingsDialog::show_page(
                    &window_widget,
                    settings_clone.clone(),
                    is_running,
                    url,
                    move |enable, port| {
                        toggle_server(enable, port);
                    },
                    move |new_org| {
                        sidebar_title.set_subtitle(new_org);
                    },
                    move |configured| {
                        action_reload_cloned.set_enabled(configured);
                        if configured {
                            let s = settings_for_cb.borrow();
                            if let Some(cfg) = DbConfig::from_env_or_settings(&s) {
                                if let Ok(db) = Database::connect(cfg) {
                                    let _ = db.init_tables();
                                    *db_conn.borrow_mut() = Some(db);
                                }
                            }
                        } else {
                            *db_conn.borrow_mut() = None;
                        }
                    },
                    page,
                );
            })
        };

        // App Action: Settings
        {
            let open_settings = open_settings.clone();
            let action_settings = gio::SimpleAction::new("settings", None);
            action_settings.connect_activate(move |_, _| {
                open_settings(Some("general"));
            });
            app.add_action(&action_settings);
        }

        // App Action: Import CSV
        {
            let contacts = contacts.clone();
            let shared_contacts = shared_contacts.clone();
            let storage = storage.clone();
            let window_widget = window.clone();
            let refresh_list = refresh_list.clone();
            let populate_locations = populate_locations.clone();
            let populate_depts = populate_depts.clone();
            let show_toast = show_toast.clone();
            let db_conn = db_conn.clone();
            let settings = settings.clone();

            let action_import = gio::SimpleAction::new("import_csv", None);
            action_import.connect_activate(move |_, _| {
                let auth_win = window_widget.clone();
                let auth_settings = settings.borrow().clone();

                let contacts = contacts.clone();
                let shared_contacts = shared_contacts.clone();
                let storage = storage.clone();
                let refresh_list = refresh_list.clone();
                let populate_locations = populate_locations.clone();
                let populate_depts = populate_depts.clone();
                let show_toast = show_toast.clone();
                let db_conn = db_conn.clone();
                let settings = settings.clone();
                let window_widget = window_widget.clone();

                prompt_security_auth(
                    &auth_win,
                    &auth_settings,
                    "Импорт контактов",
                    "Загрузка новых записей в базу данных.",
                    false,
                    move || {
                        ImportCsvDialog::show(&window_widget, move |imported, replace_mode| {
                            let count = imported.len();
                            // Если PostgreSQL подключен, сохраняем через пакетную вставку (batch insert)
                            if settings.borrow().is_db_configured() {
                                if let Ok(db) = get_or_connect_db(&db_conn, &settings.borrow()) {
                                    if let Err(e) = db.batch_insert_contacts(&imported, replace_mode) {
                                        show_toast(&format!("Ошибка импорта в PostgreSQL: {}", e));
                                    } else if let Ok(loaded) = db.fetch_all() {
                                        let mut all = contacts.borrow_mut();
                                        *all = loaded.clone();
                                        *shared_contacts.write().unwrap() = loaded.clone();
                                        let _ = storage.save_contacts(&loaded);
                                        populate_locations(&extract_locations(&contacts.borrow()));
                                        populate_depts(&extract_departments(&contacts.borrow()));
                                        refresh_list();
                                        if replace_mode {
                                            show_toast(&format!("База PostgreSQL заменена: загружено {} контактов", count));
                                        } else {
                                            show_toast(&format!("В PostgreSQL импортировано {} контактов", count));
                                        }
                                        return;
                                    }
                                }
                            }

                            // Локальное сохранение при отсутствии PostgreSQL
                            {
                                let mut all = contacts.borrow_mut();
                                if replace_mode {
                                    *all = imported;
                                    all.sort_by(|a, b| a.full_name.to_lowercase().cmp(&b.full_name.to_lowercase()));
                                } else {
                                    StorageManager::merge_contacts(&mut all, imported);
                                }
                                *shared_contacts.write().unwrap() = all.clone();
                                if let Err(err) = storage.save_contacts(&all) {
                                    show_toast(&format!("Ошибка сохранения базы: {}", err));
                                }
                            }
                            populate_locations(&extract_locations(&contacts.borrow()));
                            populate_depts(&extract_departments(&contacts.borrow()));
                            refresh_list();
                            if replace_mode {
                                show_toast(&format!("База заменена: загружено {} контактов", count));
                            } else {
                                show_toast(&format!("Импортировано {} контактов", count));
                            }
                        });
                    },
                );
            });
            app.add_action(&action_import);
        }

        // App Action: Export CSV
        {
            let contacts = contacts.clone();
            let storage = storage.clone();
            let window_widget = window.clone();
            let show_toast = show_toast.clone();
            let db_conn = db_conn.clone();
            let settings = settings.clone();

            let action_export = gio::SimpleAction::new("export_csv", None);
            action_export.connect_activate(move |_, _| {
                let auth_win = window_widget.clone();
                let auth_settings = settings.borrow().clone();

                let contacts = contacts.clone();
                let storage = storage.clone();
                let show_toast = show_toast.clone();
                let db_conn = db_conn.clone();
                let settings = settings.clone();
                let window_widget = window_widget.clone();

                prompt_security_auth(
                    &auth_win,
                    &auth_settings,
                    "Экспорт контактов",
                    "Выгрузка всех контактов в файл формата CSV.",
                    false,
                    move || {
                        let file_dialog = gtk::FileDialog::builder()
                            .title("Экспорт контактов в CSV")
                            .initial_name("contacts.csv")
                            .build();

                        let contacts = contacts.clone();
                        let storage = storage.clone();
                        let show_toast = show_toast.clone();
                        let db_conn = db_conn.clone();
                        let settings = settings.clone();

                        file_dialog.save(Some(&window_widget), None::<&gio::Cancellable>, move |res| {
                            if let Ok(file) = res {
                                if let Some(path) = file.path() {
                                    // Если подключена БД, выгружаем актуальное состояние из БД
                                    let contacts_to_export = if settings.borrow().is_db_configured() {
                                        if let Ok(db) = get_or_connect_db(&db_conn, &settings.borrow()) {
                                            db.fetch_all().unwrap_or_else(|_| contacts.borrow().clone())
                                        } else {
                                            contacts.borrow().clone()
                                        }
                                    } else {
                                        contacts.borrow().clone()
                                    };
                                    match storage.export_csv(&contacts_to_export, &path) {
                                        Ok(_) => {
                                            crate::audit::AuditLogger::log(
                                                "CONTACT_EXPORT",
                                                "local_user",
                                                &format!("Экспорт {} контактов в файл {}", contacts_to_export.len(), path.display()),
                                            );
                                            show_toast(&format!("Контакты экспортированы в {}", path.display()));
                                        }
                                        Err(err) => {
                                            crate::audit::AuditLogger::log(
                                                "EXPORT_ERROR",
                                                "local_user",
                                                &format!("Ошибка при экспорте контактов в {}: {}", path.display(), err),
                                            );
                                            show_toast(&format!("Ошибка экспорта: {}", err));
                                        }
                                    }
                                }
                            }
                        });
                    },
                );
            });
            app.add_action(&action_export);
        }

        // App Action: Clear Database
        {
            let contacts = contacts;
            let shared_contacts = shared_contacts;
            let storage = storage;
            let window_widget = window.clone();
            let refresh_list = refresh_list;
            let populate_locations = populate_locations;
            let populate_depts = populate_depts;
            let selected_contact_id = selected_contact_id;
            let details_view = details_view;
            let show_toast = show_toast;
            let db_conn = db_conn;
            let settings = settings.clone();

            let action_clear = gio::SimpleAction::new("clear_db", None);
            action_clear.connect_activate(move |_, _| {
                let dialog = adw::AlertDialog::builder()
                    .heading("Очистить базу контактов?")
                    .body("Все сохранённые и импортированные контакты будут полностью удалены. База станет пустой. Это действие нельзя отменить.")
                    .build();

                dialog.add_response("cancel", "Отмена");
                dialog.add_response("clear", "Очистить базу");
                dialog.set_response_appearance("clear", adw::ResponseAppearance::Destructive);
                dialog.set_default_response(Some("cancel"));
                dialog.set_close_response("cancel");

                let contacts = contacts.clone();
                let shared_contacts = shared_contacts.clone();
                let storage = storage.clone();
                let refresh_list = refresh_list.clone();
                let populate_locations = populate_locations.clone();
                let populate_depts = populate_depts.clone();
                let selected_contact_id = selected_contact_id.clone();
                let details_view = details_view.clone();
                let show_toast = show_toast.clone();
                let db_conn = db_conn.clone();
                let settings = settings.clone();
                let dialog_win = window_widget.clone();

                dialog.choose(&window_widget, None::<&gio::Cancellable>, move |response| {
                    if response == "clear" {
                        let auth_win = dialog_win.clone();
                        let auth_settings = settings.borrow().clone();

                        let contacts = contacts.clone();
                        let shared_contacts = shared_contacts.clone();
                        let storage = storage.clone();
                        let refresh_list = refresh_list.clone();
                        let populate_locations = populate_locations.clone();
                        let populate_depts = populate_depts.clone();
                        let selected_contact_id = selected_contact_id.clone();
                        let details_view = details_view.clone();
                        let show_toast = show_toast.clone();
                        let db_conn = db_conn.clone();
                        let settings_for_clear = settings.clone();

                        prompt_security_auth(
                            &auth_win,
                            &auth_settings,
                            "Очистка базы данных",
                            "Все сохранённые контакты будут безвозвратно удалены из базы.",
                            true,
                            move || {
                                if settings_for_clear.borrow().is_db_configured() {
                                    if let Ok(db) = get_or_connect_db(&db_conn, &settings_for_clear.borrow()) {
                                        if let Err(e) = db.clear_all() {
                                            show_toast(&format!("Ошибка очистки PostgreSQL: {}", e));
                                        }
                                    }
                                }

                                {
                                    let mut all = contacts.borrow_mut();
                                    all.clear();
                                    *shared_contacts.write().unwrap() = Vec::new();
                                    if let Err(err) = storage.save_contacts(&[]) {
                                        show_toast(&format!("Ошибка сохранения: {}", err));
                                        return;
                                    }
                                }
                                *selected_contact_id.borrow_mut() = None;
                                details_view.show_empty();
                                populate_locations(&extract_locations(&contacts.borrow()));
                                populate_depts(&extract_departments(&contacts.borrow()));
                                refresh_list();
                                crate::audit::AuditLogger::log(
                                    "DATABASE_RESET",
                                    "local_user",
                                    "Полная очистка базы контактов (все записи удалены)",
                                );
                                show_toast("База контактов полностью очищена");
                            },
                        );
                    }
                });
            });
            app.add_action(&action_clear);
        }

        // App Action: About
        {
            let open_settings = open_settings.clone();
            let action_about = gio::SimpleAction::new("about", None);
            action_about.connect_activate(move |_, _| {
                open_settings(Some("about"));
            });
            app.add_action(&action_about);
        }

        // Autostart server if enabled in settings
        if settings.borrow().web_server_enabled {
            let port = settings.borrow().web_server_port;
            toggle_server(true, port);
        }

        Self { window }
    }

    pub fn present(&self) {
        self.window.present();
    }
}

fn extract_departments(contacts_vec: &[Contact]) -> Vec<String> {
    let mut set = BTreeSet::new();
    for c in contacts_vec {
        if !c.department.trim().is_empty() {
            set.insert(c.department.clone());
        }
    }
    set.into_iter().collect()
}

fn extract_locations(contacts_vec: &[Contact]) -> Vec<String> {
    let mut set = BTreeSet::new();
    for c in contacts_vec {
        let loc = c.location.trim();
        if !loc.is_empty() {
            set.insert(loc.to_string());
        }
    }
    set.into_iter().collect()
}

fn get_or_connect_db(
    db_conn: &Rc<RefCell<Option<Database>>>,
    settings: &AppSettings,
) -> Result<Database, String> {
    if let Some(ref db) = *db_conn.borrow() {
        return Ok(db.clone());
    }

    if let Some(cfg) = DbConfig::from_env_or_settings(settings) {
        let db = Database::connect(cfg)?;
        let _ = db.init_tables();
        *db_conn.borrow_mut() = Some(db.clone());
        Ok(db)
    } else {
        Err("Настройки подключения к базе данных PostgreSQL не заданы".to_string())
    }
}


