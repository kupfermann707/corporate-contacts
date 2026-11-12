use crate::model::Contact;
use adw::prelude::*;
use std::rc::Rc;

pub struct ContactDialog;

impl ContactDialog {
    pub fn show<F>(
        parent: &impl IsA<gtk::Widget>,
        existing_contact: Option<&Contact>,
        existing_departments: &[String],
        on_save: F,
    ) where
        F: Fn(Contact) + 'static,
    {
        let is_edit = existing_contact.is_some();
        let dialog_title = if is_edit {
            "Редактирование контакта"
        } else {
            "Новый контакт"
        };

        let dialog = adw::Dialog::builder()
            .title(dialog_title)
            .content_width(480)
            .content_height(560)
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

        // Save button
        let btn_save = gtk::Button::with_label("Сохранить");
        btn_save.add_css_class("suggested-action");
        header_bar.pack_end(&btn_save);

        toolbar_view.add_top_bar(&header_bar);

        // Preferences Group: Основная информация
        let group_basic = adw::PreferencesGroup::builder()
            .title("Основная информация")
            .build();

        // Full name
        let row_name = adw::EntryRow::builder()
            .title("ФИО *")
            .build();
        if let Some(c) = existing_contact {
            row_name.set_text(&c.full_name);
        }
        group_basic.add(&row_name);

        // Location entry
        let row_location = adw::EntryRow::builder()
            .title("Локация / Филиал")
            .build();
        if let Some(c) = existing_contact {
            row_location.set_text(&c.location);
        }
        group_basic.add(&row_location);

        // Department entry
        let row_department = adw::EntryRow::builder()
            .title("Отдел")
            .build();
        if let Some(c) = existing_contact {
            row_department.set_text(&c.department);
        } else if !existing_departments.is_empty() {
            // Suggest first valid department or leave empty
        }
        group_basic.add(&row_department);

        // Position entry
        let row_position = adw::EntryRow::builder()
            .title("Должность")
            .build();
        if let Some(c) = existing_contact {
            row_position.set_text(&c.position);
        }
        group_basic.add(&row_position);

        // Preferences Group: Связь
        let group_contact = adw::PreferencesGroup::builder()
            .title("Связь и контакты")
            .build();

        let row_internal_phone = adw::EntryRow::builder()
            .title("Внутренний номер")
            .build();
        if let Some(c) = existing_contact {
            row_internal_phone.set_text(&c.internal_phone);
        }
        group_contact.add(&row_internal_phone);

        let row_mobile_phone = adw::EntryRow::builder()
            .title("Мобильный телефон")
            .build();
        if let Some(c) = existing_contact {
            row_mobile_phone.set_text(&c.mobile_phone);
        }
        group_contact.add(&row_mobile_phone);

        let row_personal_mobile = adw::SwitchRow::builder()
            .title("Личный номер (152-ФЗ)")
            .subtitle("Маскировать в общем веб-доступе (требует согласия)")
            .build();
        if let Some(c) = existing_contact {
            row_personal_mobile.set_active(c.is_personal_mobile);
        }
        group_contact.add(&row_personal_mobile);

        let row_email = adw::EntryRow::builder()
            .title("Электронная почта")
            .build();
        if let Some(c) = existing_contact {
            row_email.set_text(&c.email);
        }
        group_contact.add(&row_email);

        // Preferences Group: Размещение
        let group_place = adw::PreferencesGroup::builder()
            .title("Размещение")
            .build();

        let row_floor = adw::EntryRow::builder()
            .title("Этаж / Корпус")
            .build();
        if let Some(c) = existing_contact {
            row_floor.set_text(&c.floor);
        }
        group_place.add(&row_floor);

        let row_room = adw::EntryRow::builder()
            .title("Номер кабинета")
            .build();
        if let Some(c) = existing_contact {
            row_room.set_text(&c.office);
        }
        group_place.add(&row_room);

        // Build Preferences Page inside Clamp and ScrolledWindow
        let page = adw::PreferencesPage::new();
        page.add(&group_basic);
        page.add(&group_contact);
        page.add(&group_place);

        let clamp = adw::Clamp::builder()
            .maximum_size(480)
            .child(&page)
            .build();

        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&clamp)
            .build();

        toolbar_view.set_content(Some(&scrolled));
        dialog.set_child(Some(&toolbar_view));

        // Connect save action
        let existing_id = existing_contact.map(|c| c.id.clone());
        let existing_is_hidden = existing_contact.map(|c| c.is_hidden).unwrap_or(false);
        let on_save = Rc::new(on_save);
        let dialog_clone = dialog.clone();

        btn_save.connect_clicked(move |_| {
            let name = row_name.text().trim().to_string();
            if name.is_empty() {
                // Focus name row
                row_name.grab_focus();
                return;
            }

            let location = {
                let loc = row_location.text().trim().to_string();
                if loc.is_empty() {
                    "Основной офис".to_string()
                } else {
                    loc
                }
            };

            let department = {
                let d = row_department.text().trim().to_string();
                if d.is_empty() {
                    "Общий отдел".to_string()
                } else {
                    d
                }
            };

            let position = row_position.text().trim().to_string();
            let internal_phone = row_internal_phone.text().trim().to_string();
            let mobile_phone = row_mobile_phone.text().trim().to_string();
            let email = row_email.text().trim().to_string();
            let floor = row_floor.text().trim().to_string();
            let room = row_room.text().trim().to_string();
            let is_personal_mobile = row_personal_mobile.is_active();

            let contact = if let Some(ref id) = existing_id {
                Contact {
                    id: id.clone(),
                    location,
                    department,
                    position,
                    full_name: name,
                    internal_phone,
                    mobile_phone,
                    email,
                    floor,
                    office: room,
                    is_hidden: existing_is_hidden,
                    is_personal_mobile,
                }
            } else {
                let mut c = Contact::new(
                    location,
                    department,
                    position,
                    name,
                    internal_phone,
                    mobile_phone,
                    email,
                    floor,
                    room,
                );
                c.is_personal_mobile = is_personal_mobile;
                c
            };

            on_save(contact);
            dialog_clone.close();
        });

        dialog.present(Some(parent));
    }
}
