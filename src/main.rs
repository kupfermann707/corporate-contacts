mod audit;
mod crypto;
mod db;
mod field_mapping;
mod model;
mod qr;
mod server;
mod settings;
mod storage;
mod totp;
mod ui;

use adw::prelude::*;
use settings::AppSettings;
use ui::window::MainWindow;

const APP_ID: &str = "org.corporate.contacts";

const APP_CSS: &str = r#"
.badge-location {
    background-color: rgba(26, 115, 232, 0.14);
    color: #1a73e8;
    border-radius: 9999px;
    padding: 2px 8px;
    font-weight: 600;
    font-size: 11px;
}

@media (prefers-color-scheme: dark) {
    .badge-location {
        background-color: rgba(138, 180, 248, 0.2);
        color: #8ab4f8;
    }
}

.db-locked-blur {
    filter: blur(5px);
    opacity: 0.25;
}

.db-lock-card {
    background-color: alpha(@window_bg_color, 0.94);
    border: 1px solid alpha(@borders, 0.7);
    border-radius: 14px;
    padding: 24px 28px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.22);
}

.code-display {
    font-family: monospace;
    font-size: 15px;
    font-weight: bold;
    letter-spacing: 2px;
}

.totp-input {
    font-family: monospace;
    font-size: 18px;
    font-weight: bold;
    letter-spacing: 4px;
}

.qr-card {
    background-color: white;
    border: 2px solid alpha(@borders, 0.5);
    border-radius: 12px;
    padding: 12px;
}
"#;

fn main() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_startup(|_| {
        let provider = gtk::CssProvider::new();
        provider.load_from_string(APP_CSS);
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        // Apply saved theme
        let settings = AppSettings::load();
        settings.apply_theme();
    });

    app.connect_activate(|app| {
        let main_window = MainWindow::new(app);
        main_window.present();
    });

    app.run()
}
