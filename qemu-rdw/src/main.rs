use futures_util::StreamExt;
use gio::ApplicationFlags;
use glib::MainContext;
use gtk::{gio, glib, prelude::*};
use qemu_display::{util, Chardev, Console, Display};
use rdw::gtk;
use std::{cell::RefCell, sync::Arc};

mod audio;
mod clipboard;
mod display;
#[cfg(unix)]
mod usbredir;

struct Inner {
    app: gtk::Application,
    #[cfg(unix)]
    usbredir: RefCell<Option<usbredir::Handler>>,
    audio: RefCell<Option<audio::Handler>>,
    clipboard: RefCell<Option<clipboard::Handler>>,
}

#[derive(Clone)]
struct App {
    inner: Arc<Inner>,
}

#[derive(Debug, Default)]
struct AppOptions {
    vm_name: Option<String>,
    address: Option<String>,
    list: bool,
    wait: bool,
}

impl App {
    fn new() -> Self {
        let app = gtk::Application::new(Some("org.qemu.rdw.demo"), ApplicationFlags::NON_UNIQUE);
        app.add_main_option(
            &glib::OPTION_REMAINING,
            glib::Char(0),
            glib::OptionFlags::NONE,
            glib::OptionArg::StringArray,
            "VM name",
            Some("VM-NAME"),
        );
        app.add_main_option(
            "address",
            glib::Char(b'a' as _),
            glib::OptionFlags::NONE,
            glib::OptionArg::String,
            "D-Bus bus address",
            None,
        );
        app.add_main_option(
            "list",
            glib::Char(0),
            glib::OptionFlags::NONE,
            glib::OptionArg::None,
            "List available VM names",
            None,
        );
        app.add_main_option(
            "wait",
            glib::Char(0),
            glib::OptionFlags::NONE,
            glib::OptionArg::None,
            "Wait for display to be available",
            None,
        );
        app.add_main_option(
            "version",
            glib::Char(0),
            glib::OptionFlags::NONE,
            glib::OptionArg::None,
            "Show program version",
            None,
        );

        let opt: Arc<RefCell<AppOptions>> = Default::default();
        let opt_clone = opt.clone();
        app.connect_handle_local_options(move |_, opt| {
            let mut app_opt = opt_clone.borrow_mut();
            if opt.lookup_value("version", None).is_some() {
                println!("Version: {}", env!("CARGO_PKG_VERSION"));
                return 0;
            }
            if let Some(arg) = opt.lookup_value("address", None) {
                app_opt.address = arg.get::<String>();
            }
            if opt.lookup_value("list", None).is_some() {
                app_opt.list = true;
            }
            if opt.lookup_value("wait", None).is_some() {
                app_opt.wait = true;
            }
            app_opt.vm_name = opt
                .lookup_value(&glib::OPTION_REMAINING, None)
                .and_then(|args| args.child_value(0).get::<String>());
            -1
        });

        let app = App {
            inner: Arc::new(Inner {
                app,
                #[cfg(unix)]
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
            let opt_clone = opt.clone();
            MainContext::default().spawn_local(async move {
                let builder = if let Some(addr) = &opt_clone.borrow().address {
                    zbus::ConnectionBuilder::address(addr.as_str())
                } else {
                    zbus::ConnectionBuilder::session()
                };
                let conn = builder
                    .unwrap()
                    .internal_executor(false)
                    .build()
                    .await
                    .expect("Failed to connect to DBus");

                let conn_clone = conn.clone();
                MainContext::default().spawn_local(async move {
                    loop {
                        conn_clone.executor().tick().await;
                    }
                });

                if opt_clone.borrow().list {
                    let list = Display::by_name(&conn).await.unwrap();
                    for (name, dest) in list {
                        println!("{} (at {})", name, dest);
                    }
                    app_clone.inner.app.quit();
                    return;
                }
                let dest = {
                    let name = opt_clone.borrow().vm_name.clone();
                    let wait = opt_clone.borrow().wait;

                    Display::lookup(&conn, wait, name.as_deref()).await.unwrap()
                };

                let display = Display::new(&conn, dest).await.unwrap();

                let disp = display.clone();
                MainContext::default().spawn_local(async move {
                    let mut changed = disp.receive_owner_changed().await.unwrap();
                    while let Some(name) = changed.next().await {
                        dbg!(name);
                    }
                });

                let console = Console::new(&conn, 0)
                    .await
                    .expect("Failed to get the QEMU console");
                let rdw = display::Display::new(console);
                app_clone
                    .inner
                    .app
                    .active_window()
                    .unwrap()
                    .set_child(Some(&rdw));

                #[cfg(unix)]
                app_clone.set_usbredir(usbredir::Handler::new(display.usbredir().await));

                if let Ok(Some(audio)) = display.audio().await {
                    match audio::Handler::new(audio).await {
                        Ok(handler) => app_clone.set_audio(handler),
                        Err(e) => {
                            log::warn!("Failed to setup audio handler: {}", e);
                        }
                    }
                }

                if let Ok(Some(clipboard)) = display.clipboard().await {
                    match clipboard::Handler::new(clipboard).await {
                        Ok(handler) => app_clone.set_clipboard(handler),
                        Err(e) => {
                            log::warn!("Failed to setup clipboard handler: {}", e);
                        }
                    }
                }

                if let Ok(c) = Chardev::new(&conn, "qmp").await {
                    use std::io::{prelude::*, BufReader};
                    #[cfg(unix)]
                    use std::os::unix::net::UnixStream;
                    #[cfg(windows)]
                    use uds_windows::UnixStream;

                    let (p0, p1) = UnixStream::pair().unwrap();
                    let fd = util::prepare_uds_pass(
                        #[cfg(windows)]
                        c.proxy.inner().connection(),
                        &p1,
                    )
                    .unwrap();
                    if c.proxy.register(fd).await.is_ok() {
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

        #[cfg(unix)]
        {
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
        }

        app
    }

    #[cfg(unix)]
    fn set_usbredir(&self, usbredir: usbredir::Handler) {
        self.inner.usbredir.replace(Some(usbredir));
    }

    fn set_audio(&self, audio: audio::Handler) {
        self.inner.audio.replace(Some(audio));
    }

    fn set_clipboard(&self, cb: clipboard::Handler) {
        self.inner.clipboard.replace(Some(cb));
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
