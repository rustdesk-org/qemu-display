use crate::config;
use crate::window::QemuApplicationWindow;
use gio::ApplicationFlags;
use glib::clone;
use glib::WeakRef;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, gio, glib};
use gtk_macros::action;
use log::{debug, info};
use once_cell::sync::OnceCell;
use std::env;

use qemu_display_listener::Console;
use zbus::Connection;

mod imp {
    use super::*;
    use glib::subclass;

    #[derive(Debug)]
    pub struct QemuApplication {
        pub window: OnceCell<WeakRef<QemuApplicationWindow>>,
        pub conn: OnceCell<Connection>,
        pub addr: OnceCell<String>,
    }

    impl ObjectSubclass for QemuApplication {
        const NAME: &'static str = "QemuApplication";
        type Type = super::QemuApplication;
        type ParentType = gtk::Application;
        type Interfaces = ();
        type Instance = subclass::simple::InstanceStruct<Self>;
        type Class = subclass::simple::ClassStruct<Self>;

        glib::object_subclass!();

        fn new() -> Self {
            Self {
                window: OnceCell::new(),
                conn: OnceCell::new(),
                addr: OnceCell::new(),
            }
        }
    }

    impl ObjectImpl for QemuApplication {}

    impl gio::subclass::prelude::ApplicationImpl for QemuApplication {
        fn handle_local_options(
            &self,
            application: &Self::Type,
            options: &glib::VariantDict,
        ) -> i32 {
            if options.lookup_value("version", None).is_some() {
                println!("Version: {} ({})", config::VERSION, config::PROFILE);
                return 0;
            }

            self.parent_handle_local_options(application, options)
        }

        fn command_line(
            &self,
            application: &Self::Type,
            command_line: &gio::ApplicationCommandLine,
        ) -> i32 {
            let mut opt = command_line.get_arguments().into_iter().skip(1);
            if let Some(arg) = opt.next() {
                self.addr.set(arg.into_string().unwrap()).unwrap();
            }
            application.activate();
            self.parent_command_line(application, command_line)
        }

        fn activate(&self, app: &Self::Type) {
            debug!("GtkApplication<QemuApplication>::activate");

            if let Some(window) = self.window.get() {
                let window = window.upgrade().unwrap();
                window.show();
                window.present();
                return;
            }

            app.set_resource_base_path(Some("/org/qemu/gtk4/"));
            app.setup_css();

            let conn = if let Some(addr) = self.addr.get() {
                Connection::new_for_address(&addr, true)
            } else {
                Connection::new_session()
            }
            .expect("Failed to connect to DBus");
            let console = Console::new(&conn, 0).expect("Failed to get the console");
            self.conn.set(conn).expect("Connection already set.");

            let window = QemuApplicationWindow::new(app, console);
            self.window
                .set(window.downgrade())
                .expect("Window already set.");

            app.setup_gactions();
            app.setup_accels();

            app.get_main_window().present();
        }

        fn startup(&self, app: &Self::Type) {
            debug!("GtkApplication<QemuApplication>::startup");
            self.parent_startup(app);
        }
    }

    impl GtkApplicationImpl for QemuApplication {}
}

glib::wrapper! {
    pub struct QemuApplication(ObjectSubclass<imp::QemuApplication>)
        @extends gio::Application, gtk::Application, @implements gio::ActionMap, gio::ActionGroup;
}

impl QemuApplication {
    pub fn new() -> Self {
        let app = glib::Object::new::<Self>(&[
            ("application-id", &Some(config::APP_ID)),
            (
                "flags",
                &(ApplicationFlags::NON_UNIQUE | ApplicationFlags::HANDLES_COMMAND_LINE),
            ),
        ])
        .expect("Application initialization failed...");
        app.add_main_option(
            "version",
            glib::Char(0),
            glib::OptionFlags::NONE,
            glib::OptionArg::None,
            "Show program version",
            None,
        );
        app
    }

    fn get_main_window(&self) -> QemuApplicationWindow {
        let priv_ = imp::QemuApplication::from_instance(self);
        priv_.window.get().unwrap().upgrade().unwrap()
    }

    fn setup_gactions(&self) {
        // Quit
        action!(
            self,
            "quit",
            clone!(@weak self as app => move |_, _| {
                // This is needed to trigger the delete event
                // and saving the window state
                app.get_main_window().close();
                app.quit();
            })
        );

        // About
        action!(
            self,
            "about",
            clone!(@weak self as app => move |_, _| {
                app.show_about_dialog();
            })
        );
    }

    // Sets up keyboard shortcuts
    fn setup_accels(&self) {
        self.set_accels_for_action("app.quit", &["<primary>q"]);
        self.set_accels_for_action("win.show-help-overlay", &["<primary>question"]);
    }

    fn setup_css(&self) {
        let provider = gtk::CssProvider::new();
        provider.load_from_resource("/org/qemu/gtk4/style.css");
        if let Some(display) = gdk::Display::get_default() {
            gtk::StyleContext::add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    }

    fn show_about_dialog(&self) {
        let dialog = gtk::AboutDialogBuilder::new()
            .program_name("QEMU Gtk")
            .logo_icon_name(config::APP_ID)
            .license_type(gtk::License::MitX11)
            .website("https://gitlab.com/qemu-project/qemu/")
            .version(config::VERSION)
            .transient_for(&self.get_main_window())
            .modal(true)
            .authors(vec!["QEMU developpers".into()])
            .artists(vec!["QEMU developpers".into()])
            .build();

        dialog.show();
    }

    pub fn run(&self) {
        info!("QEMU Gtk ({})", config::APP_ID);
        info!("Version: {} ({})", config::VERSION, config::PROFILE);
        info!("Datadir: {}", config::PKGDATADIR);

        let args: Vec<String> = env::args().collect();
        ApplicationExtManual::run(self, &args);
    }
}
