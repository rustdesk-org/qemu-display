use gio::ApplicationFlags;
use gtk::{gio, prelude::*};
use qemu_display_listener::Console;
use zbus::Connection;

mod display_qemu;

fn main() {
    pretty_env_logger::init();

    let app = gtk::Application::new(Some("org.qemu.rdw.demo"), ApplicationFlags::NON_UNIQUE);

    let conn = Connection::new_session().expect("Failed to connect to DBus");

    app.connect_activate(move |app| {
        let window = gtk::ApplicationWindow::new(app);

        window.set_title(Some("rdw demo"));
        window.set_default_size(1024, 768);

        let console = Console::new(&conn, 0).expect("Failed to get the QEMU console");
        let display = display_qemu::DisplayQemu::new(console);
        window.set_child(Some(&display));

        window.show();
    });

    app.run();
}
