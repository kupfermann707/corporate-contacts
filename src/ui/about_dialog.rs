use adw::prelude::*;

#[allow(dead_code)]
pub fn show_about_dialog(parent: &impl IsA<gtk::Widget>) {
    let dialog = adw::AboutDialog::builder()
        .application_name("Справочник контактов")
        .application_icon("org.corporate.contacts")
        .developer_name("Даниил Суханов (daniilsukhanoff-hub)")
        .version("1.0.0")
        .comments("Универсальный корпоративный телефонный справочник и адресная книга контактов.\nРазработано на Rust с использованием GTK4 и Libadwaita.")
        .website("https://github.com/daniilsukhanoff-hub/corporate-contacts")
        .issue_url("https://github.com/daniilsukhanoff-hub/corporate-contacts/issues")
        .license_type(gtk::License::MitX11)
        .copyright("© 2026 Даниил Суханов")
        .developers(["Даниил Суханов (daniilsukhanoff-hub)"])
        .designers(["GNOME Design Team"])
        .build();

    dialog.present(Some(parent));
}
