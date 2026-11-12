use crate::settings::AppSettings;
use crate::totp::verify_totp;
use adw::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

pub fn prompt_security_auth<F>(
    parent: &impl IsA<gtk::Window>,
    settings: &AppSettings,
    title: &str,
    description: &str,
    destructive: bool,
    on_success: F,
) where
    F: FnOnce() + 'static,
{
    // Если ни TOTP, ни пароль БД не настроены, действие выполняется без препятствий
    if !settings.totp_enabled && settings.db_password.trim().is_empty() {
        on_success();
        return;
    }

    let is_totp = settings.totp_enabled;
    let totp_secret = settings.totp_secret.clone();
    let db_password = settings.db_password.clone();

    let window = adw::Window::builder()
        .title("Подтверждение действия")
        .modal(true)
        .transient_for(parent)
        .default_width(400)
        .default_height(460)
        .resizable(false)
        .build();

    let toolbar_view = adw::ToolbarView::new();
    let header_bar = adw::HeaderBar::builder()
        .show_end_title_buttons(true)
        .show_start_title_buttons(false)
        .build();
    toolbar_view.add_top_bar(&header_bar);

    let content_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_start(28)
        .margin_end(28)
        .margin_top(16)
        .margin_bottom(16)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .build();

    let icon = gtk::Image::from_icon_name(if is_totp {
        "channel-secure-symbolic"
    } else {
        "dialog-password-symbolic"
    });
    icon.set_pixel_size(48);
    if destructive {
        icon.add_css_class("error");
    } else {
        icon.add_css_class("accent");
    }
    content_box.append(&icon);

    let lbl_title = gtk::Label::builder()
        .label(title)
        .css_classes(["title-2"])
        .halign(gtk::Align::Center)
        .build();
    content_box.append(&lbl_title);

    let lbl_desc = gtk::Label::builder()
        .label(description)
        .wrap(true)
        .justify(gtk::Justification::Center)
        .css_classes(["dim-label"])
        .halign(gtk::Align::Center)
        .max_width_chars(32)
        .build();
    content_box.append(&lbl_desc);

    let lbl_hint = gtk::Label::builder()
        .label(if is_totp {
            "Введите 6-значный код TOTP:"
        } else {
            "Введите пароль от базы данных:"
        })
        .wrap(true)
        .justify(gtk::Justification::Center)
        .css_classes(["caption", "heading"])
        .margin_top(4)
        .halign(gtk::Align::Center)
        .build();
    content_box.append(&lbl_hint);

    let entry_totp = gtk::Entry::builder()
        .placeholder_text("123456")
        .max_length(6)
        .halign(gtk::Align::Center)
        .width_request(190)
        .css_classes(["totp-input"])
        .visible(is_totp)
        .build();
    gtk::prelude::EntryExt::set_alignment(&entry_totp, 0.5);

    let entry_pass = gtk::PasswordEntry::builder()
        .placeholder_text("Пароль от базы данных")
        .show_peek_icon(true)
        .width_request(240)
        .halign(gtk::Align::Center)
        .visible(!is_totp)
        .build();

    if is_totp {
        content_box.append(&entry_totp);
    } else {
        content_box.append(&entry_pass);
    }

    let lbl_err = gtk::Label::builder()
        .css_classes(["error", "caption"])
        .wrap(true)
        .justify(gtk::Justification::Center)
        .visible(false)
        .build();
    content_box.append(&lbl_err);

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .propagate_natural_height(true)
        .child(&content_box)
        .build();
    toolbar_view.set_content(Some(&scrolled));

    // Нижняя панель с кнопками - зафиксирована и никогда не обрезается
    let btn_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_start(24)
        .margin_end(24)
        .margin_top(12)
        .margin_bottom(16)
        .halign(gtk::Align::Center)
        .build();

    let btn_cancel = gtk::Button::builder()
        .label("Отмена")
        .width_request(110)
        .build();

    let btn_confirm = gtk::Button::builder()
        .label("Подтвердить")
        .width_request(130)
        .css_classes([if destructive { "destructive-action" } else { "suggested-action" }])
        .build();

    btn_box.append(&btn_cancel);
    btn_box.append(&btn_confirm);
    toolbar_view.add_bottom_bar(&btn_box);

    window.set_content(Some(&toolbar_view));

    // Закрытие по кнопке Отмена и клавише Escape
    let window_close = window.clone();
    btn_cancel.connect_clicked(move |_| {
        window_close.close();
    });

    let key_controller = gtk::EventControllerKey::new();
    let window_for_key = window.clone();
    key_controller.connect_key_pressed(move |_, keyval, _, _| {
        if keyval == gdk::Key::Escape {
            window_for_key.close();
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    window.add_controller(key_controller);

    let on_success_cell = Rc::new(RefCell::new(Some(on_success)));
    let window_confirm = window.clone();
    let entry_totp_val = entry_totp.clone();
    let entry_pass_val = entry_pass.clone();
    let lbl_err_val = lbl_err.clone();

    let try_confirm = move || {
        let is_valid = if is_totp {
            let code = entry_totp_val.text().trim().to_string();
            verify_totp(&totp_secret, &code)
        } else {
            let pass = entry_pass_val.text().to_string();
            pass == db_password
        };

        if is_valid {
            crate::audit::AuditLogger::log(
                "AUTH_SUCCESS",
                "local_user",
                &format!("Успешная аутентификация ({})", if is_totp { "TOTP" } else { "Пароль БД" }),
            );
            window_confirm.close();
            if let Some(callback) = on_success_cell.borrow_mut().take() {
                callback();
            }
        } else {
            crate::audit::AuditLogger::log(
                "AUTH_FAIL",
                "local_user",
                &format!("Ошибка аутентификации: неверный ввод ({})", if is_totp { "TOTP" } else { "Пароль БД" }),
            );
            lbl_err_val.set_text(if is_totp {
                "Неверный код TOTP. Проверьте время на телефоне"
            } else {
                "Неверный пароль от базы данных"
            });
            lbl_err_val.set_visible(true);
            if is_totp {
                entry_totp_val.set_text("");
            } else {
                entry_pass_val.set_text("");
            }
        }
    };

    let try_confirm_rc = Rc::new(try_confirm);

    {
        let tc = try_confirm_rc.clone();
        btn_confirm.connect_clicked(move |_| {
            tc();
        });
    }

    if is_totp {
        let tc = try_confirm_rc.clone();
        entry_totp.connect_activate(move |_| {
            tc();
        });
    } else {
        let tc = try_confirm_rc.clone();
        entry_pass.connect_activate(move |_| {
            tc();
        });
    }

    window.present();

    if is_totp {
        entry_totp.grab_focus();
    } else {
        entry_pass.grab_focus();
    }
}
