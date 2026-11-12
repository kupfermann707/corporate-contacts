use crate::model::Contact;
use adw::prelude::*;
use std::rc::Rc;

pub struct ContactDetailsView {
    container: gtk::Box,
    status_page: adw::StatusPage,
    avatar: adw::Avatar,
    name_label: gtk::Label,
    position_label: gtk::Label,
    location_badge: gtk::Label,
    hidden_badge: gtk::Label,

    // Deletion info for tickets
    deletion_group: adw::PreferencesGroup,
    deletion_id_row: adw::ActionRow,
    #[allow(dead_code)]
    deletion_copy_btn: gtk::Button,

    // Contact info rows
    internal_phone_row: adw::ActionRow,
    internal_phone_btn: gtk::Button,
    mobile_phone_row: adw::ActionRow,
    mobile_phone_btn: gtk::Button,
    email_row: adw::ActionRow,
    email_copy_btn: gtk::Button,
    email_open_btn: gtk::Button,

    // Place rows
    location_row: adw::ActionRow,
    department_row: adw::ActionRow,
    floor_row: adw::ActionRow,
    room_row: adw::ActionRow,
    current_contact: Rc<std::cell::RefCell<Option<Contact>>>,
}

impl ContactDetailsView {
    pub fn new<FToast>(toast_callback: FToast) -> Self
    where
        FToast: Fn(&str) + 'static + Clone,
    {
        let container = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .vexpand(true)
            .build();

        // Empty state
        let status_page = adw::StatusPage::builder()
            .icon_name("avatar-default-symbolic")
            .title("Справочник контактов")
            .description("Выберите контакт из списка слева для просмотра деталей или добавьте новый контакт кнопкой +")
            .hexpand(true)
            .vexpand(true)
            .build();

        // Details content box
        let content_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .margin_top(24)
            .margin_bottom(32)
            .margin_start(16)
            .margin_end(16)
            .hexpand(true)
            .build();

        // Hero Card / Banner
        let hero_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .halign(gtk::Align::Center)
            .build();

        let avatar = adw::Avatar::builder()
            .size(80)
            .show_initials(true)
            .halign(gtk::Align::Center)
            .build();

        let name_label = gtk::Label::builder()
            .css_classes(["title-1"])
            .wrap(true)
            .justify(gtk::Justification::Center)
            .halign(gtk::Align::Center)
            .selectable(true)
            .build();

        let position_label = gtk::Label::builder()
            .css_classes(["title-4", "dim-label"])
            .wrap(true)
            .justify(gtk::Justification::Center)
            .halign(gtk::Align::Center)
            .selectable(true)
            .build();

        let location_badge = gtk::Label::builder()
            .css_classes(["caption", "badge"])
            .halign(gtk::Align::Center)
            .build();

        let hidden_badge = gtk::Label::builder()
            .css_classes(["caption", "badge", "error"])
            .label("⚠️ Скрыт (помечен на удаление)")
            .halign(gtk::Align::Center)
            .visible(false)
            .build();

        hero_box.append(&avatar);
        hero_box.append(&name_label);
        hero_box.append(&position_label);
        hero_box.append(&location_badge);
        hero_box.append(&hidden_badge);

        // Group 0: Сведения о мягком удалении (для заявок)
        let deletion_group = adw::PreferencesGroup::builder()
            .title("Мягкое удаление (Soft Delete)")
            .visible(false)
            .build();

        let deletion_id_row = adw::ActionRow::builder()
            .title("Идентификатор для заявки")
            .subtitle_selectable(true)
            .build();
        deletion_id_row.add_prefix(&gtk::Image::from_icon_name("dialog-warning-symbolic"));

        let deletion_copy_btn = gtk::Button::builder()
            .icon_name("edit-copy-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text("Скопировать ID в буфер обмена для заявки")
            .css_classes(["flat"])
            .build();
        deletion_id_row.add_suffix(&deletion_copy_btn);
        deletion_group.add(&deletion_id_row);

        // Group 1: Контакты и связь
        let comm_group = adw::PreferencesGroup::builder()
            .title("Контакты и связь")
            .build();

        // Internal phone
        let internal_phone_row = adw::ActionRow::builder()
            .title("Внутренний номер")
            .build();
        internal_phone_row.add_prefix(&gtk::Image::from_icon_name("call-start-symbolic"));
        let internal_phone_btn = gtk::Button::builder()
            .icon_name("edit-copy-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text("Скопировать внутренний номер")
            .css_classes(["flat"])
            .build();
        internal_phone_row.add_suffix(&internal_phone_btn);
        comm_group.add(&internal_phone_row);

        // Mobile phone
        let mobile_phone_row = adw::ActionRow::builder()
            .title("Мобильный телефон")
            .build();
        mobile_phone_row.add_prefix(&gtk::Image::from_icon_name("phone-symbolic"));
        let mobile_phone_btn = gtk::Button::builder()
            .icon_name("edit-copy-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text("Скопировать мобильный номер")
            .css_classes(["flat"])
            .build();
        mobile_phone_row.add_suffix(&mobile_phone_btn);
        comm_group.add(&mobile_phone_row);

        // Email
        let email_row = adw::ActionRow::builder()
            .title("Электронная почта")
            .build();
        email_row.add_prefix(&gtk::Image::from_icon_name("mail-unread-symbolic"));
        let email_open_btn = gtk::Button::builder()
            .icon_name("mail-send-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text("Написать письмо")
            .css_classes(["flat"])
            .build();
        let email_copy_btn = gtk::Button::builder()
            .icon_name("edit-copy-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text("Скопировать email")
            .css_classes(["flat"])
            .build();
        email_row.add_suffix(&email_open_btn);
        email_row.add_suffix(&email_copy_btn);
        comm_group.add(&email_row);

        // Group 2: Рабочее место
        let place_group = adw::PreferencesGroup::builder()
            .title("Рабочее место")
            .build();

        let location_row = adw::ActionRow::builder()
            .title("Локация / Филиал")
            .build();
        location_row.add_prefix(&gtk::Image::from_icon_name("mark-location-symbolic"));
        place_group.add(&location_row);

        let department_row = adw::ActionRow::builder()
            .title("Отдел")
            .build();
        department_row.add_prefix(&gtk::Image::from_icon_name("system-users-symbolic"));
        place_group.add(&department_row);

        let floor_row = adw::ActionRow::builder()
            .title("Этаж / Корпус")
            .build();
        floor_row.add_prefix(&gtk::Image::from_icon_name("go-home-symbolic"));
        place_group.add(&floor_row);

        let room_row = adw::ActionRow::builder()
            .title("Номер кабинета")
            .build();
        room_row.add_prefix(&gtk::Image::from_icon_name("folder-symbolic"));
        place_group.add(&room_row);

        content_box.append(&hero_box);
        content_box.append(&deletion_group);
        content_box.append(&comm_group);
        content_box.append(&place_group);

        let clamp = adw::Clamp::builder()
            .maximum_size(650)
            .child(&content_box)
            .build();

        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&clamp)
            .hexpand(true)
            .vexpand(true)
            .build();

        container.append(&status_page);
        container.append(&scrolled);
        scrolled.set_visible(false);

        // Setup copy handlers with closures
        let current_contact: Rc<std::cell::RefCell<Option<Contact>>> = Rc::new(std::cell::RefCell::new(None));

        let cur_int = current_contact.clone();
        let toast_int = toast_callback.clone();
        internal_phone_btn.connect_clicked(move |_| {
            if let Some(ref c) = *cur_int.borrow() {
                if !c.internal_phone.is_empty() {
                    if let Some(display) = gdk::Display::default() {
                        display.clipboard().set_text(&c.internal_phone);
                        toast_int(&format!("Внутренний номер {} скопирован", c.internal_phone));
                    }
                }
            }
        });

        let cur_mob = current_contact.clone();
        let toast_mob = toast_callback.clone();
        mobile_phone_btn.connect_clicked(move |_| {
            if let Some(ref c) = *cur_mob.borrow() {
                if !c.mobile_phone.is_empty() {
                    if let Some(display) = gdk::Display::default() {
                        display.clipboard().set_text(&c.mobile_phone);
                        toast_mob(&format!("Мобильный номер {} скопирован", c.mobile_phone));
                    }
                }
            }
        });

        let cur_email_copy = current_contact.clone();
        let toast_email = toast_callback.clone();
        email_copy_btn.connect_clicked(move |_| {
            if let Some(ref c) = *cur_email_copy.borrow() {
                if !c.email.is_empty() {
                    if let Some(display) = gdk::Display::default() {
                        display.clipboard().set_text(&c.email);
                        toast_email(&format!("Email {} скопирован", c.email));
                    }
                }
            }
        });

        let cur_del_copy = current_contact.clone();
        let toast_del_copy = toast_callback.clone();
        deletion_copy_btn.connect_clicked(move |_| {
            if let Some(ref c) = *cur_del_copy.borrow() {
                if let Some(display) = gdk::Display::default() {
                    display.clipboard().set_text(&c.id);
                    toast_del_copy(&format!("ID контакта скопирован для заявки: {}", c.id));
                }
            }
        });

        let cur_email_open = current_contact.clone();
        let toast_open = toast_callback;
        email_open_btn.connect_clicked(move |_| {
            if let Some(ref c) = *cur_email_open.borrow() {
                if !c.email.is_empty() {
                    let uri = format!("mailto:{}", c.email);
                    let res = gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>);
                    if res.is_err() {
                        toast_open("Не удалось открыть почтовый клиент");
                    }
                }
            }
        });

        Self {
            container,
            status_page,
            avatar,
            name_label,
            position_label,
            location_badge,
            hidden_badge,
            deletion_group,
            deletion_id_row,
            deletion_copy_btn,
            internal_phone_row,
            internal_phone_btn,
            mobile_phone_row,
            mobile_phone_btn,
            email_row,
            email_copy_btn,
            email_open_btn,
            location_row,
            department_row,
            floor_row,
            room_row,
            current_contact,
        }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.container
    }

    pub fn show_empty(&self) {
        *self.current_contact.borrow_mut() = None;
        self.status_page.set_visible(true);
        self.hidden_badge.set_visible(false);
        self.deletion_group.set_visible(false);
        if let Some(scrolled) = self.container.last_child() {
            scrolled.set_visible(false);
        }
    }

    pub fn update(&self, contact: Option<&Contact>) {
        *self.current_contact.borrow_mut() = contact.cloned();
        if let Some(c) = contact {
            self.status_page.set_visible(false);
            if let Some(scrolled) = self.container.last_child() {
                scrolled.set_visible(true);
            }

            self.avatar.set_text(Some(&c.full_name));

            if c.is_hidden {
                self.name_label.set_use_markup(true);
                self.name_label.set_markup(&format!(
                    "<span alpha='65%'><s>{}</s></span>",
                    glib::markup_escape_text(&c.full_name)
                ));
                self.name_label.add_css_class("dim-label");
                self.hidden_badge.set_visible(true);
                self.deletion_id_row.set_subtitle(&format!("ID: {} (для заявки в IT/Helpdesk)", c.id));
                self.deletion_group.set_visible(true);
            } else {
                self.name_label.set_use_markup(false);
                self.name_label.remove_css_class("dim-label");
                self.name_label.set_text(&c.full_name);
                self.hidden_badge.set_visible(false);
                self.deletion_group.set_visible(false);
            }
            self.position_label.set_text(if c.position.is_empty() { "—" } else { &c.position });

            // Location badge
            if !c.location.trim().is_empty() {
                self.location_badge.set_text(&format!("📍 {}", c.location));
                self.location_badge.set_visible(true);
            } else {
                self.location_badge.set_visible(false);
            }
            self.location_badge.add_css_class("badge-location");

            self.location_row.set_subtitle(&c.location);

            // Internal phone
            let int_phone = if c.internal_phone.is_empty() { "Не указан" } else { &c.internal_phone };
            self.internal_phone_row.set_subtitle(int_phone);
            self.internal_phone_btn.set_sensitive(!c.internal_phone.is_empty());

            // Mobile phone
            let mob_phone = if c.mobile_phone.is_empty() { "Не указан" } else { &c.mobile_phone };
            self.mobile_phone_row.set_subtitle(mob_phone);
            self.mobile_phone_btn.set_sensitive(!c.mobile_phone.is_empty());

            // Email
            let email_str = if c.email.is_empty() { "Не указан" } else { &c.email };
            self.email_row.set_subtitle(email_str);
            let has_email = !c.email.is_empty();
            self.email_copy_btn.set_sensitive(has_email);
            self.email_open_btn.set_sensitive(has_email);

            // Department, floor, room/office
            self.department_row.set_subtitle(if c.department.is_empty() { "—" } else { &c.department });
            self.floor_row.set_subtitle(if c.floor.is_empty() { "—" } else { &c.floor });
            self.room_row.set_subtitle(if c.office.is_empty() { "—" } else { &c.office });
        } else {
            self.show_empty();
        }
    }
}
