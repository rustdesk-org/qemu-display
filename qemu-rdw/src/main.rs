use gio::ApplicationFlags;
use glib::{clone, MainContext};
use gtk::{gio, glib, prelude::*};
use once_cell::sync::OnceCell;
use qemu_display_listener::{Chardev, Console};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use zbus::Connection;

mod audio;
mod clipboard;
mod display_qemu;

fn main() {
    pretty_env_logger::init();

    let app = gtk::Application::new(Some("org.qemu.rdw.demo"), ApplicationFlags::NON_UNIQUE);

    let conn: zbus::azync::Connection = Connection::new_session()
        .expect("Failed to connect to DBus")
        .into();

    let audio = std::sync::Arc::new(OnceCell::new());
    let clipboard = std::sync::Arc::new(OnceCell::new());

    app.connect_activate(move |app| {
        let window = gtk::ApplicationWindow::new(app);

        window.set_title(Some("rdw demo"));
        window.set_default_size(1024, 768);

        let conn = conn.clone();
        let audio_clone = audio.clone();
        let clipboard_clone = clipboard.clone();
        MainContext::default().spawn_local(clone!(@strong window => async move {
            let console = Console::new(&conn, 0).await.expect("Failed to get the QEMU console");
            let display = display_qemu::DisplayQemu::new(console);
            window.set_child(Some(&display));

            match audio::Handler::new(&conn).await {
                Ok(handler) => audio_clone.set(handler).unwrap(),
                Err(e) => log::warn!("Failed to setup audio: {}", e),
            }

            match clipboard::Handler::new(&conn).await {
                Ok(handler) => clipboard_clone.set(handler).unwrap(),
                Err(e) => log::warn!("Failed to setup clipboard: {}", e),
            }

            if let Ok(c) = Chardev::new(&conn, "qmp").await {
                use std::io::BufReader;
                use std::io::prelude::*;

                let (p0, p1) = UnixStream::pair().unwrap();
                if c.proxy.register(p1.as_raw_fd().into()).await.is_ok() {
                    let mut reader = BufReader::new(p0.try_clone().unwrap());
                    let mut line = String::new();
                    std::thread::spawn(move || loop {
                        if reader.read_line(&mut line).unwrap() > 0 {
                            println!("{}", &line);
                        }
                    });
                }
            }

            window.show();
        }));
    });

    app.run();
}
