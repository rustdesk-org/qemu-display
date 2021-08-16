use gio::ApplicationFlags;
use glib::MainContext;
use gtk::{gio, glib, prelude::*};
use qemu_display::{Chardev, Console, Display};
use std::cell::RefCell;
use std::sync::Arc;
use zbus::Connection;

mod audio;
mod clipboard;
mod display;
mod usbredir;

struct Inner {
    app: gtk::Application,
    conn: zbus::azync::Connection,
    usbredir: RefCell<Option<usbredir::Handler>>,
    audio: RefCell<Option<audio::Handler>>,
    clipboard: RefCell<Option<clipboard::Handler>>,
}

#[derive(Clone)]
struct App {
    inner: Arc<Inner>,
}

impl App {
    fn new() -> Self {
        let app = gtk::Application::new(Some("org.qemu.rdw.demo"), ApplicationFlags::NON_UNIQUE);
        let conn = Connection::session()
            .expect("Failed to connect to DBus")
            .into();

        let app = App {
            inner: Arc::new(Inner {
                app,
                conn,
                usbredir: Default::default(),
                audio: Default::default(),
                clipboard: Default::default(),
            }),
        };

        let app_clone = app.clone();
        app.inner.app.connect_activate(move |app| {
            let ui_src = include_str!("main.ui");
            let builder = gtk::Builder::new();
            builder
                .add_from_string(ui_src)
                .expect("Couldn't add from string");
            let window: gtk::ApplicationWindow =
                builder.object("window").expect("Couldn't get window");
            window.set_application(Some(app));

            let app_clone = app_clone.clone();
            MainContext::default().spawn_local(async move {
                let display = Display::new(app_clone.connection()).await.unwrap();

                let console = Console::new(app_clone.connection(), 0)
                    .await
                    .expect("Failed to get the QEMU console");
                let rdw = display::Display::new(console);
                app_clone
                    .inner
                    .app
                    .active_window()
                    .unwrap()
                    .set_child(Some(&rdw));

                app_clone.set_usbredir(usbredir::Handler::new(display.usbredir().await));

                if let Ok(Some(audio)) = display.audio().await {
                    match audio::Handler::new(audio).await {
                        Ok(handler) => app_clone.set_audio(handler),
                        Err(e) => log::warn!("Failed to setup audio: {}", e),
                    }
                }

                if let Ok(Some(clipboard)) = display.clipboard().await {
                    match clipboard::Handler::new(clipboard).await {
                        Ok(handler) => app_clone.set_clipboard(handler),
                        Err(e) => log::warn!("Failed to setup clipboard: {}", e),
                    }
                }

                if let Ok(c) = Chardev::new(app_clone.connection(), "qmp").await {
                    use std::io::prelude::*;
                    use std::io::BufReader;
                    use std::os::unix::io::AsRawFd;
                    use std::os::unix::net::UnixStream;

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
            });
        });

        let action_usb = gio::SimpleAction::new("usb", None);
        let app_clone = app.clone();
        action_usb.connect_activate(move |_, _| {
            let usbredir = app_clone.inner.usbredir.borrow();
            if let Some(usbredir) = usbredir.as_ref() {
                let dialog = gtk::Dialog::new();
                dialog.set_transient_for(app_clone.inner.app.active_window().as_ref());
                dialog.set_child(Some(&usbredir.widget()));
                dialog.show();
            }
        });
        app.inner.app.add_action(&action_usb);

        app
    }

    fn connection(&self) -> &zbus::azync::Connection {
        &self.inner.conn
    }

    fn set_usbredir(&self, usbredir: usbredir::Handler) {
        self.inner.usbredir.replace(Some(usbredir));
    }

    fn set_audio(&self, audio: audio::Handler) {
        self.inner.audio.replace(Some(audio));
    }

    fn set_clipboard(&self, clipboard: clipboard::Handler) {
        self.inner.clipboard.replace(Some(clipboard));
    }

    fn run(&self) -> i32 {
        self.inner.app.run()
    }
}

fn main() {
    pretty_env_logger::init();

    let app = App::new();
    app.run();
}
