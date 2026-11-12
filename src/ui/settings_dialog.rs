use crate::db::{Database, DbConfig};
use crate::server::get_local_ip;
use crate::qr::qr_to_texture;
use crate::settings::AppSettings;
use crate::totp::{format_totp_uri, generate_totp_secret, verify_totp};
use adw::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

pub struct SettingsDialog;

impl SettingsDialog {
    #[allow(dead_code)]
    pub fn show<FToggleServer, FOrgChanged, FDbChanged>(
        parent: &impl IsA<gtk::Window>,
        settings: Rc<RefCell<AppSettings>>,
        is_server_running: bool,
        current_server_url: Option<String>,
        on_toggle_server: FToggleServer,
        on_org_changed: FOrgChanged,
        on_db_changed: FDbChanged,
    ) where
        FToggleServer: Fn(bool, u16) + 'static + Clone,
        FOrgChanged: Fn(&str) + 'static + Clone,
        FDbChanged: Fn(bool) + 'static + Clone,
    {
        Self::show_page(
            parent,
            settings,
            is_server_running,
            current_server_url,
            on_toggle_server,
            on_org_changed,
            on_db_changed,
            None,
        );
    }

    pub fn show_page<FToggleServer, FOrgChanged, FDbChanged>(
        parent: &impl IsA<gtk::Window>,
        settings: Rc<RefCell<AppSettings>>,
        is_server_running: bool,
        current_server_url: Option<String>,
        on_toggle_server: FToggleServer,
        on_org_changed: FOrgChanged,
        on_db_changed: FDbChanged,
        initial_page: Option<&str>,
    ) where
        FToggleServer: Fn(bool, u16) + 'static + Clone,
        FOrgChanged: Fn(&str) + 'static + Clone,
        FDbChanged: Fn(bool) + 'static + Clone,
    {
        let window = adw::PreferencesWindow::builder()
            .title("Настройки")
            .modal(true)
            .transient_for(parent)
            .default_width(620)
            .default_height(680)
            .build();

        // =========================================================================
        // Вкладка 1: Основные настройки (General)
        // =========================================================================
        let page_general = adw::PreferencesPage::builder()
            .title("Настройки")
            .icon_name("preferences-other-symbolic")
            .name("general")
            .build();

        // ---------------- Group: Web Server ----------------
        let group_server = adw::PreferencesGroup::builder()
            .title("Веб-сервер в локальной сети (LAN)")
            .description("Позволяет открыть доступ к справочнику через любой браузер для коллег в сети компании")
            .build();

        let switch_server = adw::SwitchRow::builder()
            .title("Включить веб-доступ в локальной сети")
            .subtitle("Справочник будет доступен всем подключенным к сети пользователям")
            .active(is_server_running)
            .build();
        group_server.add(&switch_server);

        let row_url = adw::ActionRow::builder()
            .title("Адрес для подключения")
            .subtitle(if is_server_running {
                current_server_url.as_deref().unwrap_or("http://...")
            } else {
                "Веб-сервер выключен"
            })
            .build();
        row_url.add_prefix(&gtk::Image::from_icon_name("network-wired-symbolic"));

        let btn_copy = gtk::Button::builder()
            .icon_name("edit-copy-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text("Скопировать адрес")
            .sensitive(is_server_running)
            .css_classes(["flat"])
            .build();
        row_url.add_suffix(&btn_copy);
        group_server.add(&row_url);

        let cur_port = settings.borrow().web_server_port;
        let row_port = adw::SpinRow::builder()
            .title("Порт сервера")
            .subtitle("Рекомендуется 8443 для HTTPS (152-ФЗ) или 8080 для HTTP")
            .adjustment(&gtk::Adjustment::new(
                cur_port as f64,
                1024.0,
                65535.0,
                1.0,
                10.0,
                0.0,
            ))
            .numeric(true)
            .sensitive(!is_server_running)
            .build();
        group_server.add(&row_port);

        let cur_https = settings.borrow().https_enabled;
        let switch_https = adw::SwitchRow::builder()
            .title("Шифрование HTTPS (152-ФЗ / TLS)")
            .subtitle("Обязательно по ст. 19 152-ФЗ и Приказу ФСТЭК № 21 для защиты ПДн в сети")
            .active(cur_https)
            .sensitive(!is_server_running)
            .build();
        switch_https.add_prefix(&gtk::Image::from_icon_name("channel-secure-symbolic"));
        group_server.add(&switch_https);

        let cur_cert = settings.borrow().tls_cert_path.clone();
        let row_cert = adw::EntryRow::builder()
            .title("TLS Сертификат (.pem / .crt)")
            .text(&cur_cert)
            .sensitive(!is_server_running)
            .build();
        group_server.add(&row_cert);

        let cur_key = settings.borrow().tls_key_path.clone();
        let row_key = adw::EntryRow::builder()
            .title("TLS Приватный ключ (.pem / .key)")
            .text(&cur_key)
            .sensitive(!is_server_running)
            .build();
        group_server.add(&row_key);

        let row_gen_cert = adw::ActionRow::builder()
            .title("Локальный TLS-сертификат 152-ФЗ")
            .subtitle("Создать или обновить локальный самоподписанный сертификат (OpenSSL)")
            .build();
        let btn_gen_cert = gtk::Button::builder()
            .label("Сгенерировать")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .sensitive(!is_server_running)
            .build();
        row_gen_cert.add_suffix(&btn_gen_cert);
        group_server.add(&row_gen_cert);

        // Copy button handler
        {
            let row_url = row_url.clone();
            btn_copy.connect_clicked(move |_| {
                if let Some(subtitle) = row_url.subtitle() {
                    if let Some(display) = gdk::Display::default() {
                        display.clipboard().set_text(&subtitle);
                    }
                }
            });
        }

        // Switch server handler
        {
            let row_url = row_url.clone();
            let btn_copy = btn_copy.clone();
            let row_port = row_port.clone();
            let on_toggle = on_toggle_server.clone();
            let settings = settings.clone();

            switch_server.connect_active_notify(move |switch| {
                let active = switch.is_active();
                let port = row_port.value() as u16;

                // Update settings
                {
                    let mut s = settings.borrow_mut();
                    s.web_server_enabled = active;
                    s.web_server_port = port;
                    let _ = s.save();
                }

                row_port.set_sensitive(!active);
                btn_copy.set_sensitive(active);

                if active {
                    let ip = get_local_ip();
                    let is_https = settings.borrow().https_enabled;
                    let scheme = if is_https { "https" } else { "http" };
                    let url = format!("{}://{}:{}", scheme, ip, port);
                    row_url.set_subtitle(&url);
                } else {
                    row_url.set_subtitle("Веб-сервер выключен");
                }

                on_toggle(active, port);
            });
        }

        // HTTPS switch handler
        {
            let settings = settings.clone();
            let row_port = row_port.clone();
            switch_https.connect_active_notify(move |sw| {
                let active = sw.is_active();
                let mut s = settings.borrow_mut();
                s.https_enabled = active;
                if active && s.web_server_port == 8080 {
                    s.web_server_port = 8443;
                    row_port.set_value(8443.0);
                } else if !active && s.web_server_port == 8443 {
                    s.web_server_port = 8080;
                    row_port.set_value(8080.0);
                }
                let _ = s.save();
            });
        }

        // Cert & Key handlers
        {
            let settings = settings.clone();
            row_cert.connect_changed(move |entry| {
                let text = entry.text().to_string();
                let mut s = settings.borrow_mut();
                s.tls_cert_path = text;
                let _ = s.save();
            });
        }
        {
            let settings = settings.clone();
            row_key.connect_changed(move |entry| {
                let text = entry.text().to_string();
                let mut s = settings.borrow_mut();
                s.tls_key_path = text;
                let _ = s.save();
            });
        }

        // Generate cert button handler
        {
            let row_cert = row_cert.clone();
            let row_key = row_key.clone();
            let settings = settings.clone();
            btn_gen_cert.connect_clicked(move |btn| {
                let default_cert = crate::settings::default_tls_cert_path();
                let default_key = crate::settings::default_tls_key_path();
                let cp = std::path::PathBuf::from(&default_cert);
                let kp = std::path::PathBuf::from(&default_key);

                let _ = std::fs::remove_file(&cp);
                let _ = std::fs::remove_file(&kp);

                match crate::server::ensure_tls_certificates(&cp, &kp) {
                    Ok(()) => {
                        let mut s = settings.borrow_mut();
                        s.tls_cert_path = default_cert.clone();
                        s.tls_key_path = default_key.clone();
                        let _ = s.save();

                        row_cert.set_text(&default_cert);
                        row_key.set_text(&default_key);
                        btn.set_label("Создан ✓");
                    }
                    Err(e) => {
                        btn.set_label("Ошибка!");
                        eprintln!("Ошибка генерации TLS-сертификата: {}", e);
                    }
                }
            });
        }

        // Port change handler
        {
            let settings = settings.clone();
            row_port.connect_value_notify(move |spin| {
                let port = spin.value() as u16;
                let mut s = settings.borrow_mut();
                s.web_server_port = port;
                let _ = s.save();
            });
        }

        // ---------------- Group: Appearance ----------------
        let group_appearance = adw::PreferencesGroup::builder()
            .title("Оформление и организация")
            .build();

        let row_org = adw::EntryRow::builder()
            .title("Название организации / филиала")
            .text(&settings.borrow().organization_name)
            .build();
        group_appearance.add(&row_org);

        {
            let on_org_changed = on_org_changed.clone();
            let settings = settings.clone();
            row_org.connect_changed(move |entry| {
                let text = entry.text().trim().to_string();
                let org = if text.is_empty() {
                    "Справочник контактов".to_string()
                } else {
                    text
                };
                {
                    let mut s = settings.borrow_mut();
                    s.organization_name = org.clone();
                    let _ = s.save();
                }
                on_org_changed(&org);
            });
        }

        // Theme selection
        let cur_theme = settings.borrow().theme.clone();
        let theme_selected = match cur_theme.as_str() {
            "light" => 1,
            "dark" => 2,
            _ => 0,
        };
        let row_theme = adw::ComboRow::builder()
            .title("Цветовая тема")
            .model(&gtk::StringList::new(&[
                "Системная",
                "Светлая",
                "Тёмная",
            ]))
            .selected(theme_selected)
            .build();
        group_appearance.add(&row_theme);

        {
            let settings = settings.clone();
            row_theme.connect_selected_notify(move |combo| {
                let theme_str = match combo.selected() {
                    1 => "light",
                    2 => "dark",
                    _ => "system",
                };
                {
                    let mut s = settings.borrow_mut();
                    s.theme = theme_str.to_string();
                    let _ = s.save();
                    s.apply_theme();
                }
            });
        }

        let row_location = adw::EntryRow::builder()
            .title("Локация по умолчанию для новых контактов")
            .text(&settings.borrow().default_location)
            .build();
        group_appearance.add(&row_location);

        {
            let settings = settings.clone();
            row_location.connect_changed(move |entry| {
                let loc = entry.text().trim().to_string();
                let mut s = settings.borrow_mut();
                s.default_location = if loc.is_empty() { "Все".to_string() } else { loc };
                let _ = s.save();
            });
        }

        // ---------------- Group: PostgreSQL Database ----------------
        let group_db = adw::PreferencesGroup::builder()
            .title("База данных PostgreSQL")
            .description("Централизованное хранилище контактов организации")
            .build();

        // Кнопка принудительной блокировки в заголовке группы
        let btn_relock = gtk::Button::builder()
            .icon_name("channel-secure-symbolic")
            .tooltip_text("Заблокировать раздел базы данных")
            .css_classes(["flat"])
            .valign(gtk::Align::Center)
            .visible(false)
            .build();
        group_db.set_header_suffix(Some(&btn_relock));

        // Строка IP + Порт
        let row_db = adw::PreferencesRow::builder()
            .activatable(false)
            .selectable(false)
            .build();

        let hbox_db = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .margin_start(16)
            .margin_end(16)
            .margin_top(10)
            .margin_bottom(10)
            .valign(gtk::Align::Center)
            .build();

        let ip_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .hexpand(true)
            .build();
        let lbl_ip = gtk::Label::builder()
            .label("IP-адрес / хост")
            .css_classes(["caption", "dim-label"])
            .halign(gtk::Align::Start)
            .build();
        let entry_ip = gtk::Entry::builder()
            .placeholder_text("192.168.1.100 или localhost")
            .text(&settings.borrow().db_host)
            .hexpand(true)
            .build();
        ip_box.append(&lbl_ip);
        ip_box.append(&entry_ip);

        let port_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .width_request(110)
            .build();
        let lbl_port = gtk::Label::builder()
            .label("Порт")
            .css_classes(["caption", "dim-label"])
            .halign(gtk::Align::Start)
            .build();
        let entry_port = gtk::Entry::builder()
            .placeholder_text("5432")
            .text(&settings.borrow().db_port)
            .build();
        port_box.append(&lbl_port);
        port_box.append(&entry_port);

        hbox_db.append(&ip_box);
        hbox_db.append(&port_box);
        row_db.set_child(Some(&hbox_db));

        let row_dbname = adw::EntryRow::builder()
            .title("Имя базы данных (DB_NAME)")
            .text(&settings.borrow().db_name)
            .build();

        let row_dbuser = adw::EntryRow::builder()
            .title("Пользователь (DB_USER)")
            .text(&settings.borrow().db_user)
            .build();

        let row_dbpass = adw::PasswordEntryRow::builder()
            .title("Пароль (DB_PASSWORD)")
            .text(&settings.borrow().db_password)
            .build();

        let is_configured = settings.borrow().is_db_configured();
        let row_status = adw::ActionRow::builder()
            .title("Синхронизация")
            .subtitle(if is_configured {
                "✓ Подключение настроено. Кнопка «Обновить базу» активна"
            } else {
                "Укажите IP и порт, чтобы активировать кнопку «Обновить базу»"
            })
            .build();
        row_status.add_prefix(&gtk::Image::from_icon_name("network-server-symbolic"));

        let btn_test_conn = gtk::Button::builder()
            .label("Проверить")
            .valign(gtk::Align::Center)
            .tooltip_text("Проверить соединение с базой данных PostgreSQL")
            .css_classes(["flat"])
            .build();
        row_status.add_suffix(&btn_test_conn);

        // Список полей БД в стандартном виде Libadwaita boxed-list
        let db_list_box = gtk::ListBox::builder()
            .css_classes(["boxed-list"])
            .selection_mode(gtk::SelectionMode::None)
            .build();
        db_list_box.append(&row_db);
        db_list_box.append(&row_dbname);
        db_list_box.append(&row_dbuser);
        db_list_box.append(&row_dbpass);
        db_list_box.append(&row_status);

        // Контейнер Overlay для эффекта размытия и плавающей карточки разблокировки
        let overlay = gtk::Overlay::builder()
            .child(&db_list_box)
            .build();

        let lock_card = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .margin_start(24)
            .margin_end(24)
            .margin_top(20)
            .margin_bottom(20)
            .css_classes(["db-lock-card"])
            .build();

        let lock_icon = gtk::Image::from_icon_name("channel-secure-symbolic");
        lock_icon.set_pixel_size(44);
        lock_card.append(&lock_icon);

        let lbl_lock_title = gtk::Label::builder()
            .label("Раздел базы данных заблокирован")
            .css_classes(["title-3"])
            .build();
        lock_card.append(&lbl_lock_title);

        let lbl_lock_desc = gtk::Label::builder()
            .wrap(true)
            .justify(gtk::Justification::Center)
            .css_classes(["dim-label"])
            .build();
        lock_card.append(&lbl_lock_desc);

        // Поле ввода TOTP (6 цифр)
        let entry_totp = gtk::Entry::builder()
            .placeholder_text("123456")
            .max_length(6)
            .halign(gtk::Align::Center)
            .width_request(180)
            .css_classes(["totp-input"])
            .build();
        lock_card.append(&entry_totp);

        // Поле ввода пароля от БД
        let entry_pass = gtk::PasswordEntry::builder()
            .placeholder_text("Пароль от базы данных")
            .show_peek_icon(true)
            .width_request(240)
            .halign(gtk::Align::Center)
            .build();
        lock_card.append(&entry_pass);

        let btn_unlock = gtk::Button::builder()
            .label("Разблокировать")
            .css_classes(["suggested-action"])
            .halign(gtk::Align::Center)
            .build();
        lock_card.append(&btn_unlock);

        let lbl_lock_err = gtk::Label::builder()
            .css_classes(["error", "caption"])
            .wrap(true)
            .justify(gtk::Justification::Center)
            .visible(false)
            .build();
        lock_card.append(&lbl_lock_err);

        overlay.add_overlay(&lock_card);
        group_db.add(&overlay);

        // Замыкание обновления визуального состояния блокировки БД
        let update_lock_view = {
            let settings = settings.clone();
            let db_list_box = db_list_box.clone();
            let lock_card = lock_card.clone();
            let btn_relock = btn_relock.clone();
            let lbl_lock_desc = lbl_lock_desc.clone();
            let entry_totp = entry_totp.clone();
            let entry_pass = entry_pass.clone();
            let lbl_lock_err = lbl_lock_err.clone();

            Rc::new(move |locked: bool| {
                let s = settings.borrow();
                if !s.is_db_configured() {
                    // БД не настроена -> показываем поля открытыми для первого ввода
                    db_list_box.remove_css_class("db-locked-blur");
                    db_list_box.set_sensitive(true);
                    lock_card.set_visible(false);
                    btn_relock.set_visible(false);
                    return;
                }

                if locked {
                    db_list_box.add_css_class("db-locked-blur");
                    db_list_box.set_sensitive(false);
                    lock_card.set_visible(true);
                    btn_relock.set_visible(false);
                    lbl_lock_err.set_visible(false);
                    entry_totp.set_text("");
                    entry_pass.set_text("");

                    if s.totp_enabled {
                        lbl_lock_desc.set_text(
                            "Для доступа введите 6-значный код TOTP из Authenticator",
                        );
                        entry_totp.set_visible(true);
                        entry_pass.set_visible(false);
                    } else {
                        lbl_lock_desc.set_text(
                            "Для доступа введите пароль от базы данных",
                        );
                        entry_totp.set_visible(false);
                        entry_pass.set_visible(true);
                    }
                } else {
                    db_list_box.remove_css_class("db-locked-blur");
                    db_list_box.set_sensitive(true);
                    lock_card.set_visible(false);
                    btn_relock.set_visible(true);
                }
            })
        };

        // Начальная инициализация состояния блокировки
        update_lock_view(is_configured);

        // Обработчик кнопки принудительной повторной блокировки
        {
            let update_lock_view = update_lock_view.clone();
            btn_relock.connect_clicked(move |_| {
                update_lock_view(true);
            });
        }

        // Логика разблокировки БД (по TOTP коду или по паролю БД)
        let perform_unlock = {
            let settings = settings.clone();
            let update_lock_view = update_lock_view.clone();
            let entry_totp = entry_totp.clone();
            let entry_pass = entry_pass.clone();
            let lbl_lock_err = lbl_lock_err.clone();

            Rc::new(move || {
                let s = settings.borrow();
                if s.totp_enabled {
                    let code = entry_totp.text().trim().to_string();
                    if verify_totp(&s.totp_secret, &code) {
                        update_lock_view(false);
                    } else {
                        lbl_lock_err.set_text("Неверный 6-значный код TOTP. Попробуйте снова");
                        lbl_lock_err.set_visible(true);
                        entry_totp.set_text("");
                    }
                } else {
                    let pass = entry_pass.text().to_string();
                    if pass == s.db_password {
                        update_lock_view(false);
                    } else {
                        lbl_lock_err.set_text("Неверный пароль от базы данных");
                        lbl_lock_err.set_visible(true);
                        entry_pass.set_text("");
                    }
                }
            })
        };

        {
            let perform_unlock = perform_unlock.clone();
            btn_unlock.connect_clicked(move |_| {
                perform_unlock();
            });
        }
        {
            let perform_unlock = perform_unlock.clone();
            entry_totp.connect_activate(move |_| {
                perform_unlock();
            });
        }
        {
            let perform_unlock = perform_unlock.clone();
            entry_pass.connect_activate(move |_| {
                perform_unlock();
            });
        }

        // Проверка подключения к БД
        {
            let settings = settings.clone();
            let row_status = row_status.clone();
            let on_db_changed = on_db_changed.clone();
            btn_test_conn.connect_clicked(move |_| {
                let s = settings.borrow();
                if let Some(cfg) = DbConfig::from_env_or_settings(&s) {
                    row_status.set_subtitle("Проверка подключения...");
                    match Database::connect(cfg) {
                        Ok(db) => {
                            let _ = db.init_tables();
                            row_status.set_subtitle("✓ Соединение с PostgreSQL успешно установлено!");
                            on_db_changed(true);
                        }
                        Err(e) => {
                            row_status.set_subtitle(&format!("Ошибка подключения: {}", e));
                            on_db_changed(false);
                        }
                    }
                } else {
                    row_status.set_subtitle("Укажите IP-адрес и порт для проверки");
                }
            });
        }

        // Отслеживание изменений полей БД
        {
            let settings = settings.clone();
            let entry_port = entry_port.clone();
            let row_status = row_status.clone();
            let on_db_changed = on_db_changed.clone();

            entry_ip.connect_changed(move |entry| {
                let ip_val = entry.text().trim().to_string();
                let port_val = entry_port.text().trim().to_string();
                let configured = !ip_val.is_empty() && !port_val.is_empty();
                {
                    let mut s = settings.borrow_mut();
                    s.db_host = ip_val;
                    let _ = s.save();
                }
                if configured {
                    row_status.set_subtitle("✓ Подключение настроено. Кнопка «Обновить базу» активна");
                } else {
                    row_status.set_subtitle("Укажите IP и порт, чтобы активировать кнопку «Обновить базу»");
                }
                on_db_changed(configured);
            });
        }

        {
            let settings = settings.clone();
            let entry_ip = entry_ip.clone();
            let row_status = row_status.clone();
            let on_db_changed = on_db_changed.clone();

            entry_port.connect_changed(move |entry| {
                let port_val = entry.text().trim().to_string();
                let ip_val = entry_ip.text().trim().to_string();
                let configured = !ip_val.is_empty() && !port_val.is_empty();
                {
                    let mut s = settings.borrow_mut();
                    s.db_port = port_val;
                    let _ = s.save();
                }
                if configured {
                    row_status.set_subtitle("✓ Подключение настроено. Кнопка «Обновить базу» активна");
                } else {
                    row_status.set_subtitle("Укажите IP и порт, чтобы активировать кнопку «Обновить базу»");
                }
                on_db_changed(configured);
            });
        }

        {
            let settings = settings.clone();
            let on_db_changed = on_db_changed.clone();
            row_dbname.connect_changed(move |entry| {
                let configured = {
                    let mut s = settings.borrow_mut();
                    s.db_name = entry.text().trim().to_string();
                    let _ = s.save();
                    s.is_db_configured()
                };
                on_db_changed(configured);
            });
        }

        {
            let settings = settings.clone();
            let on_db_changed = on_db_changed.clone();
            row_dbuser.connect_changed(move |entry| {
                let configured = {
                    let mut s = settings.borrow_mut();
                    s.db_user = entry.text().trim().to_string();
                    let _ = s.save();
                    s.is_db_configured()
                };
                on_db_changed(configured);
            });
        }

        {
            let settings = settings.clone();
            let on_db_changed = on_db_changed.clone();
            row_dbpass.connect_changed(move |entry| {
                let configured = {
                    let mut s = settings.borrow_mut();
                    s.db_password = entry.text().to_string();
                    let _ = s.save();
                    s.is_db_configured()
                };
                on_db_changed(configured);
            });
        }

        page_general.add(&group_server);
        page_general.add(&group_db);
        page_general.add(&group_appearance);
        window.add(&page_general);

        // =========================================================================
        // Вкладка 2: О программе и настройка TOTP (About)
        // =========================================================================
        let page_about = adw::PreferencesPage::builder()
            .title("О программе")
            .icon_name("help-about-symbolic")
            .name("about")
            .build();

        // Группа: Информация о программе
        let group_about_app = adw::PreferencesGroup::builder()
            .title("О программе")
            .build();

        let row_app_title = adw::ActionRow::builder()
            .title("Справочник контактов")
            .subtitle("Версия 1.0.0 • Телефонный корпоративный справочник")
            .build();
        row_app_title.add_prefix(&gtk::Image::from_icon_name("org.corporate.contacts"));
        let link_gh_badge = gtk::LinkButton::builder()
            .uri("https://github.com/daniilsukhanoff-hub/corporate-contacts")
            .label("GitHub")
            .valign(gtk::Align::Center)
            .build();
        row_app_title.add_suffix(&link_gh_badge);
        group_about_app.add(&row_app_title);

        let row_app_desc = adw::ActionRow::builder()
            .title("Описание")
            .subtitle("Универсальный корпоративный телефонный справочник контактов на Rust, GTK4 и Libadwaita")
            .build();
        group_about_app.add(&row_app_desc);

        let row_app_license = adw::ActionRow::builder()
            .title("Лицензия")
            .subtitle("MIT License • 2026")
            .build();
        group_about_app.add(&row_app_license);
        page_about.add(&group_about_app);

        // Группа: TOTP двухфакторная аутентификация
        let group_totp = adw::PreferencesGroup::builder()
            .title("Двухфакторная аутентификация (TOTP)")
            .description("Защита доступа к настройкам базы данных одноразовыми 6-значными кодами Authenticator")
            .build();

        let is_totp_active = settings.borrow().totp_enabled;
        let row_totp_status = adw::ActionRow::builder()
            .title("Статус двухфакторной защиты")
            .subtitle(if is_totp_active {
                "✓ Включена (код запрашивается при входе в параметры базы данных)"
            } else {
                "Отключена (параметры базы данных защищены паролем БД)"
            })
            .build();
        let img_totp_status = gtk::Image::from_icon_name(if is_totp_active {
            "security-high-symbolic"
        } else {
            "security-low-symbolic"
        });
        row_totp_status.add_prefix(&img_totp_status);
        group_totp.add(&row_totp_status);

        // Строка авторизации по паролю от базы данных для доступа к настройкам TOTP
        let row_auth_pass = adw::PasswordEntryRow::builder()
            .title("Пароль от БД для управления TOTP")
            .build();

        let btn_auth_totp = gtk::Button::builder()
            .label("Открыть настройки TOTP")
            .valign(gtk::Align::Center)
            .css_classes(["suggested-action"])
            .build();
        row_auth_pass.add_suffix(&btn_auth_totp);
        group_totp.add(&row_auth_pass);

        let lbl_auth_err = gtk::Label::builder()
            .css_classes(["error", "caption"])
            .halign(gtk::Align::Start)
            .margin_start(16)
            .margin_top(4)
            .wrap(true)
            .visible(false)
            .build();
        group_totp.add(&lbl_auth_err);

        // Контейнер управления TOTP (открывается ТОЛЬКО после верного пароля от базы данных)
        let group_totp_manage = adw::PreferencesGroup::builder()
            .title("Параметры аутентификатора TOTP")
            .visible(false)
            .build();

        let candidate_secret = Rc::new(RefCell::new(generate_totp_secret()));

        // QR-код для сканирования камерой в Authenticator
        let picture_qr = gtk::Picture::builder()
            .can_shrink(false)
            .width_request(190)
            .height_request(190)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();

        let qr_frame = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .halign(gtk::Align::Center)
            .margin_top(8)
            .margin_bottom(8)
            .css_classes(["qr-card"])
            .build();
        qr_frame.append(&picture_qr);

        let lbl_qr_hint = gtk::Label::builder()
            .label("Отсканируйте QR-код в Google Authenticator, Яндекс Ключ или 1Password:")
            .css_classes(["caption", "dim-label"])
            .halign(gtk::Align::Center)
            .wrap(true)
            .justify(gtk::Justification::Center)
            .build();

        let qr_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .margin_top(4)
            .margin_bottom(8)
            .halign(gtk::Align::Center)
            .build();
        qr_box.append(&lbl_qr_hint);
        qr_box.append(&qr_frame);

        let row_qr = adw::PreferencesRow::builder()
            .activatable(false)
            .selectable(false)
            .child(&qr_box)
            .build();
        group_totp_manage.add(&row_qr);

        // Карточка с ключом и ссылкой
        let row_secret_key = adw::ActionRow::builder()
            .title("Секретный ключ Base32")
            .subtitle(&*candidate_secret.borrow())
            .build();
        let btn_copy_secret = gtk::Button::builder()
            .icon_name("edit-copy-symbolic")
            .tooltip_text("Скопировать ключ в буфер обмена")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        row_secret_key.add_suffix(&btn_copy_secret);
        group_totp_manage.add(&row_secret_key);

        let row_uri = adw::ActionRow::builder()
            .title("Ссылка для приложения (otpauth URI)")
            .subtitle("Нажмите кнопку справа, чтобы скопировать ссылку")
            .build();
        let btn_copy_uri = gtk::Button::builder()
            .icon_name("edit-copy-symbolic")
            .tooltip_text("Скопировать URI для аутентификатора")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        row_uri.add_suffix(&btn_copy_uri);
        group_totp_manage.add(&row_uri);

        let row_verify_code = adw::EntryRow::builder()
            .title("Проверочный 6-значный код из приложения")
            .build();
        group_totp_manage.add(&row_verify_code);

        let lbl_activate_msg = gtk::Label::builder()
            .css_classes(["caption"])
            .halign(gtk::Align::Start)
            .margin_start(16)
            .margin_top(4)
            .wrap(true)
            .visible(false)
            .build();
        group_totp_manage.add(&lbl_activate_msg);

        // Кнопки действий TOTP
        let hbox_totp_actions = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .margin_top(8)
            .margin_bottom(8)
            .halign(gtk::Align::Start)
            .build();

        let btn_activate_totp = gtk::Button::builder()
            .label("Активировать TOTP")
            .css_classes(["suggested-action"])
            .build();
        hbox_totp_actions.append(&btn_activate_totp);

        let btn_regen_totp = gtk::Button::builder()
            .label("Сгенерировать новый ключ")
            .build();
        hbox_totp_actions.append(&btn_regen_totp);

        let btn_disable_totp = gtk::Button::builder()
            .label("Отключить TOTP")
            .css_classes(["destructive-action"])
            .visible(is_totp_active)
            .build();
        hbox_totp_actions.append(&btn_disable_totp);

        let btn_lock_totp = gtk::Button::builder()
            .label("Заблокировать настройки")
            .css_classes(["flat"])
            .tooltip_text("Скрыть параметры TOTP и сбросить авторизацию")
            .build();
        hbox_totp_actions.append(&btn_lock_totp);

        let row_actions_wrapper = adw::PreferencesRow::builder()
            .activatable(false)
            .selectable(false)
            .child(&hbox_totp_actions)
            .build();
        group_totp_manage.add(&row_actions_wrapper);

        // Обновление отображения настроек TOTP и QR-кода
        let update_totp_mgmt_view = {
            let settings = settings.clone();
            let candidate_secret = candidate_secret.clone();
            let row_totp_status = row_totp_status.clone();
            let img_totp_status = img_totp_status.clone();
            let row_secret_key = row_secret_key.clone();
            let row_uri = row_uri.clone();
            let picture_qr = picture_qr.clone();
            let btn_activate_totp = btn_activate_totp.clone();
            let btn_disable_totp = btn_disable_totp.clone();
            let row_verify_code = row_verify_code.clone();
            let lbl_activate_msg = lbl_activate_msg.clone();
            let update_lock_view = update_lock_view.clone();

            Rc::new(move || {
                let s = settings.borrow();
                let is_active = s.totp_enabled;

                let (sec, uri) = if is_active {
                    let sec = s.totp_secret.clone();
                    let uri = format_totp_uri(&sec, "admin", "Contacts");
                    (sec, uri)
                } else {
                    let sec = candidate_secret.borrow().clone();
                    let uri = format_totp_uri(&sec, "admin", "Contacts");
                    (sec, uri)
                };

                row_secret_key.set_subtitle(&sec);
                row_uri.set_subtitle(&uri);

                if let Ok(texture) = qr_to_texture(&uri, 190) {
                    picture_qr.set_paintable(Some(&texture));
                }

                if is_active {
                    row_totp_status.set_subtitle(
                        "✓ Включена (код запрашивается при входе в параметры базы данных)",
                    );
                    img_totp_status.set_icon_name(Some("security-high-symbolic"));
                    btn_disable_totp.set_visible(true);
                    btn_activate_totp.set_visible(false);
                    row_verify_code.set_visible(false);
                } else {
                    row_totp_status.set_subtitle(
                        "Отключена (параметры базы данных защищены паролем БД)",
                    );
                    img_totp_status.set_icon_name(Some("security-low-symbolic"));
                    btn_disable_totp.set_visible(false);
                    btn_activate_totp.set_visible(true);
                    row_verify_code.set_visible(true);
                }
                lbl_activate_msg.set_visible(false);
                update_lock_view(s.is_db_configured());
            })
        };

        // Копирование секретного ключа
        {
            let candidate_secret = candidate_secret.clone();
            let settings = settings.clone();
            let lbl_activate_msg = lbl_activate_msg.clone();
            btn_copy_secret.connect_clicked(move |_| {
                let s = settings.borrow();
                let sec = if s.totp_enabled {
                    s.totp_secret.clone()
                } else {
                    candidate_secret.borrow().clone()
                };
                if let Some(display) = gdk::Display::default() {
                    display.clipboard().set_text(&sec);
                }
                lbl_activate_msg.set_text("✓ Секретный ключ скопирован в буфер обмена");
                lbl_activate_msg.remove_css_class("error");
                lbl_activate_msg.set_visible(true);
            });
        }

        // Копирование URI
        {
            let candidate_secret = candidate_secret.clone();
            let settings = settings.clone();
            let lbl_activate_msg = lbl_activate_msg.clone();
            btn_copy_uri.connect_clicked(move |_| {
                let s = settings.borrow();
                let sec = if s.totp_enabled {
                    s.totp_secret.clone()
                } else {
                    candidate_secret.borrow().clone()
                };
                let uri = format_totp_uri(&sec, "admin", "Contacts");
                if let Some(display) = gdk::Display::default() {
                    display.clipboard().set_text(&uri);
                }
                lbl_activate_msg.set_text("✓ Ссылка otpauth:// скопирована в буфер обмена");
                lbl_activate_msg.remove_css_class("error");
                lbl_activate_msg.set_visible(true);
            });
        }

        // Генерация нового ключа
        {
            let candidate_secret = candidate_secret.clone();
            let row_secret_key = row_secret_key.clone();
            let row_uri = row_uri.clone();
            let picture_qr = picture_qr.clone();
            let btn_activate_totp = btn_activate_totp.clone();
            let row_verify_code = row_verify_code.clone();
            let lbl_activate_msg = lbl_activate_msg.clone();

            btn_regen_totp.connect_clicked(move |_| {
                let new_sec = generate_totp_secret();
                *candidate_secret.borrow_mut() = new_sec.clone();
                row_secret_key.set_subtitle(&new_sec);
                let uri = format_totp_uri(&new_sec, "admin", "Contacts");
                row_uri.set_subtitle(&uri);

                if let Ok(texture) = qr_to_texture(&uri, 190) {
                    picture_qr.set_paintable(Some(&texture));
                }

                btn_activate_totp.set_visible(true);
                row_verify_code.set_visible(true);
                row_verify_code.set_text("");
                lbl_activate_msg.set_text("Сгенерирован новый QR-код и ключ. Отсканируйте его и введите 6-значный код для активации");
                lbl_activate_msg.remove_css_class("error");
                lbl_activate_msg.set_visible(true);
            });
        }

        // Активация TOTP по коду: после успеха блокирует перенастройку и сбрасывает пароль от БД
        {
            let settings = settings.clone();
            let candidate_secret = candidate_secret.clone();
            let row_verify_code = row_verify_code.clone();
            let lbl_activate_msg = lbl_activate_msg.clone();
            let group_totp_manage = group_totp_manage.clone();
            let row_auth_pass = row_auth_pass.clone();
            let btn_auth_totp = btn_auth_totp.clone();
            let row_totp_status = row_totp_status.clone();
            let img_totp_status = img_totp_status.clone();
            let lbl_auth_err = lbl_auth_err.clone();
            let update_lock_view = update_lock_view.clone();

            btn_activate_totp.connect_clicked(move |_| {
                let code = row_verify_code.text().trim().to_string();
                let sec = candidate_secret.borrow().clone();

                if verify_totp(&sec, &code) {
                    {
                        let mut s = settings.borrow_mut();
                        s.totp_enabled = true;
                        s.totp_secret = sec;
                        let _ = s.save();
                    }

                    // 1. Блокируем возможность перенастройки: скрываем панель параметров TOTP
                    group_totp_manage.set_visible(false);

                    // 2. Сбрасываем введенный пароль от базы данных
                    row_auth_pass.set_text("");
                    row_auth_pass.set_sensitive(true);
                    btn_auth_totp.set_sensitive(true);
                    btn_auth_totp.set_label("Открыть настройки TOTP");

                    // 3. Обновляем статус активности
                    row_totp_status.set_subtitle(
                        "✓ Включена (код запрашивается при входе в параметры базы данных)",
                    );
                    img_totp_status.set_icon_name(Some("security-high-symbolic"));

                    // 4. Показываем статусное сообщение об успешной активации и блокировке
                    lbl_auth_err.set_text("✓ Двухфакторная аутентификация (TOTP) успешно включена! Настройки заблокированы. Для повторного доступа требуется ввести пароль от БД.");
                    lbl_auth_err.remove_css_class("error");
                    lbl_auth_err.set_visible(true);

                    // 5. Блокируем раздел БД в первой вкладке со значком замка и запросом 6-значного TOTP
                    update_lock_view(true);
                } else {
                    lbl_activate_msg.set_text("Неверный 6-значный код. Проверьте время на телефоне и попробуйте снова");
                    lbl_activate_msg.add_css_class("error");
                    lbl_activate_msg.set_visible(true);
                }
            });
        }

        // Отключение TOTP: также блокирует панель и сбрасывает пароль от БД
        {
            let settings = settings.clone();
            let group_totp_manage = group_totp_manage.clone();
            let row_auth_pass = row_auth_pass.clone();
            let btn_auth_totp = btn_auth_totp.clone();
            let row_totp_status = row_totp_status.clone();
            let img_totp_status = img_totp_status.clone();
            let lbl_auth_err = lbl_auth_err.clone();
            let update_lock_view = update_lock_view.clone();

            btn_disable_totp.connect_clicked(move |_| {
                {
                    let mut s = settings.borrow_mut();
                    s.totp_enabled = false;
                    s.totp_secret.clear();
                    let _ = s.save();
                }

                // Блокируем панель и сбрасываем введенный пароль от БД
                group_totp_manage.set_visible(false);
                row_auth_pass.set_text("");
                row_auth_pass.set_sensitive(true);
                btn_auth_totp.set_sensitive(true);
                btn_auth_totp.set_label("Открыть настройки TOTP");

                row_totp_status.set_subtitle("Отключена (параметры базы данных защищены паролем БД)");
                img_totp_status.set_icon_name(Some("security-low-symbolic"));

                lbl_auth_err.set_text("✓ Двухфакторная аутентификация отключена.");
                lbl_auth_err.remove_css_class("error");
                lbl_auth_err.set_visible(true);

                update_lock_view(settings.borrow().is_db_configured());
            });
        }

        // Кнопка блокировки панели управления TOTP
        {
            let group_totp_manage = group_totp_manage.clone();
            let row_auth_pass = row_auth_pass.clone();
            let btn_auth_totp = btn_auth_totp.clone();
            let lbl_auth_err = lbl_auth_err.clone();

            btn_lock_totp.connect_clicked(move |_| {
                group_totp_manage.set_visible(false);
                row_auth_pass.set_text("");
                row_auth_pass.set_sensitive(true);
                btn_auth_totp.set_sensitive(true);
                btn_auth_totp.set_label("Открыть настройки TOTP");
                lbl_auth_err.set_visible(false);
            });
        }

        // Проверка пароля от БД для открытия раздела настроек TOTP
        let auth_totp_action = {
            let settings = settings.clone();
            let row_auth_pass = row_auth_pass.clone();
            let btn_auth_totp = btn_auth_totp.clone();
            let lbl_auth_err = lbl_auth_err.clone();
            let group_totp_manage = group_totp_manage.clone();
            let update_totp_mgmt_view = update_totp_mgmt_view.clone();

            Rc::new(move || {
                let entered = row_auth_pass.text().to_string();
                let s = settings.borrow();
                let real_pass = s.db_password.clone();

                if !s.is_db_configured() || real_pass.is_empty() {
                    lbl_auth_err.set_text(
                        "Сначала укажите и сохраните пароль от базы данных во вкладке «Настройки»",
                    );
                    lbl_auth_err.set_visible(true);
                    return;
                }

                if entered == real_pass {
                    lbl_auth_err.set_visible(false);
                    row_auth_pass.set_sensitive(false);
                    btn_auth_totp.set_sensitive(false);
                    btn_auth_totp.set_label("✓ Доступ открыт");
                    group_totp_manage.set_visible(true);
                    update_totp_mgmt_view();
                } else {
                    lbl_auth_err.set_text(
                        "Неверный пароль от базы данных. Доступ к настройкам TOTP запрещён",
                    );
                    lbl_auth_err.set_visible(true);
                }
            })
        };

        {
            let auth_totp_action = auth_totp_action.clone();
            btn_auth_totp.connect_clicked(move |_| {
                auth_totp_action();
            });
        }
        {
            let auth_totp_action = auth_totp_action.clone();
            row_auth_pass.connect_entry_activated(move |_| {
                auth_totp_action();
            });
        }

        page_about.add(&group_totp);
        page_about.add(&group_totp_manage);
        window.add(&page_about);

        // Закрытие окна настроек
        {
            let settings = settings.clone();
            let on_db_changed = on_db_changed.clone();
            window.connect_close_request(move |_| {
                let configured = settings.borrow().is_db_configured();
                on_db_changed(configured);
                glib::Propagation::Proceed
            });
        }

        // Переход на выбранную вкладку, если указана
        if let Some(page_name) = initial_page {
            window.set_visible_page_name(page_name);
        }

        window.present();
    }
}
