use crate::model::Contact;
use crate::storage::StorageManager;
use adw::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

pub struct ImportCsvDialog;

impl ImportCsvDialog {
    pub fn show<F>(parent: &impl IsA<gtk::Window>, on_import: F)
    where
        F: Fn(Vec<Contact>, bool) + 'static,
    {
        let parent_window = parent.as_ref().clone().upcast::<gtk::Window>();
        let parent_widget = parent.as_ref().clone().upcast::<gtk::Widget>();
        let dialog = adw::Dialog::builder()
            .title("Импорт контактов из CSV")
            .content_width(520)
            .content_height(620)
            .build();

        let toolbar_view = adw::ToolbarView::new();
        let header_bar = adw::HeaderBar::new();

        // Cancel button
        let btn_cancel = gtk::Button::with_label("Отмена");
        let d_clone = dialog.clone();
        btn_cancel.connect_clicked(move |_| {
            d_clone.close();
        });
        header_bar.pack_start(&btn_cancel);

        // Import action button (initially insensitive until valid file is selected)
        let btn_import = gtk::Button::with_label("Импортировать");
        btn_import.add_css_class("suggested-action");
        btn_import.set_sensitive(false);
        header_bar.pack_end(&btn_import);

        toolbar_view.add_top_bar(&header_bar);

        // Main content box inside clamp and scrolled window
        let content_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(16)
            .margin_top(16)
            .margin_bottom(24)
            .margin_start(16)
            .margin_end(16)
            .build();

        // ---------------- GROUP 1: Читаемые поля CSV ----------------
        let group_fields = adw::PreferencesGroup::builder()
            .title("Читаемые поля CSV-файла")
            .description("Приложение автоматически считывает следующие поля (по именам столбцов или их порядку):")
            .build();

        let add_field_row = |title: &str, subtitle: &str, icon: &str, is_required: bool| {
            let row = adw::ActionRow::builder()
                .title(title)
                .subtitle(subtitle)
                .build();
            row.add_prefix(&gtk::Image::from_icon_name(icon));
            if is_required {
                let badge = gtk::Label::builder()
                    .label("Обязательное")
                    .css_classes(["badge-location"])
                    .valign(gtk::Align::Center)
                    .build();
                row.add_suffix(&badge);
            }
            row
        };

        group_fields.add(&add_field_row(
            "ФИО",
            "Фамилия, Имя, Отчество (заголовки: ФИО, Имя, Full Name, Name)",
            "avatar-default-symbolic",
            true,
        ));
        group_fields.add(&add_field_row(
            "Локация / Филиал",
            "Город, филиал или площадка компании",
            "mark-location-symbolic",
            false,
        ));
        group_fields.add(&add_field_row(
            "Отдел",
            "Подразделение компании (заголовки: Отдел, Подразделение, Department)",
            "system-users-symbolic",
            false,
        ));
        group_fields.add(&add_field_row(
            "Должность",
            "Занимаемая должность (заголовки: Должность, Позиция, Position, Title)",
            "view-list-symbolic",
            false,
        ));
        group_fields.add(&add_field_row(
            "Внутренний телефон",
            "Короткий внутренний номер (заголовки: Внутренний номер, Внутр., Ext)",
            "call-start-symbolic",
            false,
        ));
        group_fields.add(&add_field_row(
            "Мобильный телефон",
            "Номер мобильного или городского телефона (Телефон, Мобильный, Phone)",
            "phone-symbolic",
            false,
        ));
        group_fields.add(&add_field_row(
            "Электронная почта",
            "Email адрес (Email, E-mail, Почта)",
            "mail-unread-symbolic",
            false,
        ));
        group_fields.add(&add_field_row(
            "Этаж / Корпус",
            "Номер этажа или корпуса рабочего места",
            "go-home-symbolic",
            false,
        ));
        group_fields.add(&add_field_row(
            "Кабинет",
            "Номер кабинета или комнаты",
            "folder-symbolic",
            false,
        ));

        let format_hint = gtk::Label::builder()
            .label("Поддерживаются разделители: точка с запятой (;), запятая (,), табуляция.\nКодировка: UTF-8 (с BOM или без) и Windows-1251.")
            .css_classes(["dim-label", "caption"])
            .wrap(true)
            .halign(gtk::Align::Start)
            .margin_start(6)
            .margin_top(4)
            .build();
        group_fields.add(&format_hint);

        // ---------------- GROUP 2: Выбор файла и режим импорта ----------------
        let group_import = adw::PreferencesGroup::builder()
            .title("Параметры импорта")
            .build();

        let row_file = adw::ActionRow::builder()
            .title("Файл для импорта")
            .subtitle("Файл не выбран")
            .build();
        row_file.add_prefix(&gtk::Image::from_icon_name("text-x-generic-symbolic"));

        let btn_browse = gtk::Button::builder()
            .label("Выбрать файл...")
            .icon_name("document-open-symbolic")
            .valign(gtk::Align::Center)
            .build();
        row_file.add_suffix(&btn_browse);
        group_import.add(&row_file);

        let mode_list = gtk::StringList::new(&[
            "Добавить к текущей базе (обновить существующие)",
            "Заменить текущую базу полностью",
        ]);
        let row_mode = adw::ComboRow::builder()
            .title("Режим загрузки")
            .subtitle("Выберите способ объединения с текущей базой")
            .model(&mode_list)
            .selected(0)
            .build();
        group_import.add(&row_mode);

        let row_status = adw::ActionRow::builder()
            .title("Статус анализа")
            .subtitle("Нажмите «Выбрать файл...» для проверки данных")
            .build();
        row_status.add_prefix(&gtk::Image::from_icon_name("dialog-information-symbolic"));
        group_import.add(&row_status);

        content_box.append(&group_fields);
        content_box.append(&group_import);

        let clamp = adw::Clamp::builder()
            .maximum_size(500)
            .child(&content_box)
            .build();

        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&clamp)
            .vexpand(true)
            .build();

        toolbar_view.set_content(Some(&scrolled));
        dialog.set_child(Some(&toolbar_view));

        // State for parsed contacts
        let parsed_contacts: Rc<RefCell<Option<Vec<Contact>>>> = Rc::new(RefCell::new(None));

        // Browse button handler
        {
            let parent_window = parent_window;
            let row_file = row_file.clone();
            let row_status = row_status.clone();
            let btn_import = btn_import.clone();
            let parsed_contacts = parsed_contacts.clone();

            btn_browse.connect_clicked(move |_| {
                let file_dialog = gtk::FileDialog::builder()
                    .title("Выберите CSV-файл для импорта")
                    .build();

                let filter = gtk::FileFilter::new();
                filter.set_name(Some("CSV файлы (*.csv)"));
                filter.add_pattern("*.csv");
                filter.add_pattern("*.CSV");
                filter.add_pattern("*.txt");
                filter.add_mime_type("text/csv");
                filter.add_mime_type("text/plain");

                let filters = gio::ListStore::new::<gtk::FileFilter>();
                filters.append(&filter);
                file_dialog.set_filters(Some(&filters));

                let row_file = row_file.clone();
                let row_status = row_status.clone();
                let btn_import = btn_import.clone();
                let parsed_contacts = parsed_contacts.clone();

                file_dialog.open(Some(&parent_window), None::<&gio::Cancellable>, move |res| {
                    if let Ok(file) = res {
                        if let Some(path) = file.path() {
                            row_file.set_subtitle(&path.display().to_string());
                            match StorageManager::parse_csv_file(&path) {
                                Ok((contacts, report)) => {
                                    if contacts.is_empty() {
                                        row_status.set_subtitle("В файле не найдено строк с контактами");
                                        btn_import.set_sensitive(false);
                                        *parsed_contacts.borrow_mut() = None;
                                    } else {
                                        let cols_str = report.recognized_columns.join(", ");
                                        row_status.set_subtitle(&format!(
                                            "✓ Найдено записей: {}\nРазделитель: «{}» | Распознано: {}",
                                            contacts.len(),
                                            report.delimiter,
                                            cols_str
                                        ));
                                        btn_import.set_sensitive(true);
                                        *parsed_contacts.borrow_mut() = Some(contacts);
                                    }
                                }
                                Err(err) => {
                                    row_status.set_subtitle(&format!("Ошибка чтения: {}", err));
                                    btn_import.set_sensitive(false);
                                    *parsed_contacts.borrow_mut() = None;
                                }
                            }
                        }
                    }
                });
            });
        }

        // Import button handler
        {
            let dialog_clone = dialog.clone();
            let parsed_contacts = parsed_contacts.clone();
            let row_mode = row_mode.clone();

            btn_import.connect_clicked(move |_| {
                if let Some(contacts) = parsed_contacts.borrow_mut().take() {
                    let replace = row_mode.selected() == 1;
                    on_import(contacts, replace);
                    dialog_clone.close();
                }
            });
        }

        dialog.present(Some(&parent_widget));
    }
}
