use futures::prelude::*;
use glib::{clone, MainContext};
use gtk::{gio, glib};
use qemu_display::Chardev;
use std::os::unix::{io::AsRawFd, net::UnixStream};
use vte::{gtk, prelude::*};
use zbus::Connection;

fn main() {
    pretty_env_logger::init();
    let chardev_id = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "serial0".to_string());

    let app = gtk::Application::new(Some("org.qemu.vte-example"), Default::default());
    app.add_main_option(
        &glib::OPTION_REMAINING,
        glib::Char(0),
        glib::OptionFlags::NONE,
        glib::OptionArg::StringArray,
        "ID",
        Some("chardev-name/id"),
    );
    app.connect_handle_local_options(|_, _| -1);
    app.connect_activate(move |app| {
        let window = gtk::ApplicationWindow::new(app);
        window.set_title(Some("D-Bus serial example"));
        let term = vte::Terminal::new();
        window.set_child(Some(&term));

        let id = chardev_id.clone();
        MainContext::default().spawn_local(clone!(@strong window => async move {
            let conn = Connection::session().await
                .expect("Failed to connect to session D-Bus");

            let c = Chardev::new(&conn, &id).await.unwrap();
            c.proxy.name().await.expect("Chardev not found");

            let (p0, p1) = UnixStream::pair().unwrap();
            if c.proxy.register(p1.as_raw_fd().into()).await.is_ok() {
                let ostream = unsafe { gio::UnixOutputStream::with_fd(p0.as_raw_fd()) };
                let istream = unsafe { gio::UnixInputStream::take_fd(p0) }
                    .dynamic_cast::<gio::PollableInputStream>()
                    .unwrap();

                let mut read = istream.into_async_read().unwrap();
                term.connect_commit(move |_, text, _| {
                    let _res = ostream.write(text.as_bytes(), gio::Cancellable::NONE); // TODO cancellable and error
                });

                loop {
                    let mut buffer = [0u8; 8192];
                    match read.read(&mut buffer[..]).await {
                        Ok(0) => break,
                        Ok(len) => {
                            term.feed(&buffer[..len]);
                        }
                        Err(e) => {
                            log::warn!("{}", e);
                            break;
                        }
                    }
                }
            }
        }));

        window.show();
    });

    app.run();
}
