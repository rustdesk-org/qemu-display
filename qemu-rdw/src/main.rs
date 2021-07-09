use gio::ApplicationFlags;
use glib::{clone, MainContext};
use gtk::{gio, glib, prelude::*};
use once_cell::sync::OnceCell;
use qemu_display_listener::Console;
use zbus::Connection;

mod audio;
mod display_qemu;

fn main() {
    pretty_env_logger::init();

    let app = gtk::Application::new(Some("org.qemu.rdw.demo"), ApplicationFlags::NON_UNIQUE);

    let conn: zbus::azync::Connection = Connection::new_session()
        .expect("Failed to connect to DBus")
        .into();

    let audio = std::sync::Arc::new(OnceCell::new());

    app.connect_activate(move |app| {
        let window = gtk::ApplicationWindow::new(app);

        window.set_title(Some("rdw demo"));
        window.set_default_size(1024, 768);

        let conn = conn.clone();
        let audio_clone = audio.clone();
        MainContext::default().spawn_local(clone!(@strong window => async move {
            let console = Console::new(&conn, 0).await.expect("Failed to get the QEMU console");
            let display = display_qemu::DisplayQemu::new(console);
            window.set_child(Some(&display));

            match audio::Handler::new(&conn).await {
                Ok(handler) => audio_clone.set(handler).unwrap(),
                Err(e) => log::warn!("Failed to setup audio: {}", e),
            }

            window.show();
        }));
    });

    app.run();
}
