use crate::application::QemuApplication;
use crate::config::{APP_ID, PROFILE};
use glib::clone;
use glib::signal::Inhibit;
use gtk::subclass::prelude::*;
use gtk::{self, prelude::*};
use gtk::{gio, glib, CompositeTemplate};
use log::warn;

use qemu_display_listener::Console;

mod imp {
    use super::*;
    use glib::subclass;

    #[derive(Debug, CompositeTemplate)]
    #[template(resource = "/org/qemu/gtk4/window.ui")]
    pub struct QemuApplicationWindow {
        #[template_child]
        pub headerbar: TemplateChild<gtk::HeaderBar>,
        #[template_child]
        pub label: TemplateChild<gtk::Label>,
        pub settings: gio::Settings,
    }

    impl ObjectSubclass for QemuApplicationWindow {
        const NAME: &'static str = "QemuApplicationWindow";
        type Type = super::QemuApplicationWindow;
        type ParentType = gtk::ApplicationWindow;
        type Interfaces = ();
        type Instance = subclass::simple::InstanceStruct<Self>;
        type Class = subclass::simple::ClassStruct<Self>;

        glib::object_subclass!();

        fn new() -> Self {
            Self {
                headerbar: TemplateChild::default(),
                label: TemplateChild::default(),
                settings: gio::Settings::new(APP_ID),
            }
        }

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        // You must call `Widget`'s `init_template()` within `instance_init()`.
        fn instance_init(obj: &glib::subclass::InitializingObject<Self::Type>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for QemuApplicationWindow {
        fn constructed(&self, obj: &Self::Type) {
            self.parent_constructed(obj);

            let builder = gtk::Builder::from_resource("/org/qemu/gtk4/shortcuts.ui");
            let shortcuts = builder.get_object("shortcuts").unwrap();
            obj.set_help_overlay(Some(&shortcuts));

            // Devel Profile
            if PROFILE == "Devel" {
                obj.get_style_context().add_class("devel");
            }

            // load latest window state
            obj.load_window_size();
        }
    }

    impl WindowImpl for QemuApplicationWindow {
        // save window state on delete event
        fn close_request(&self, obj: &Self::Type) -> Inhibit {
            if let Err(err) = obj.save_window_size() {
                warn!("Failed to save window state, {}", &err);
            }
            Inhibit(false)
        }
    }

    impl WidgetImpl for QemuApplicationWindow {}
    impl ApplicationWindowImpl for QemuApplicationWindow {}
}

glib::wrapper! {
    pub struct QemuApplicationWindow(ObjectSubclass<imp::QemuApplicationWindow>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, @implements gio::ActionMap, gio::ActionGroup;
}

impl QemuApplicationWindow {
    pub fn new(app: &QemuApplication, console: Console) -> Self {
        let window: Self = glib::Object::new(&[]).expect("Failed to create QemuApplicationWindow");
        window.set_application(Some(app));

        // Set icons for shell
        gtk::Window::set_default_icon_name(APP_ID);

        let rx = console
            .glib_listen()
            .expect("Failed to listen to the console");
        rx.attach(
            None,
            clone!(@weak window as win => move |t| {
                let label = &imp::QemuApplicationWindow::from_instance(&win).label;
                label.set_text(&format!("{:?}", t));
                Continue(true)
            }),
        );

        window
    }

    pub fn save_window_size(&self) -> Result<(), glib::BoolError> {
        let settings = &imp::QemuApplicationWindow::from_instance(self).settings;

        let size = self.get_default_size();

        settings.set_int("window-width", size.0)?;
        settings.set_int("window-height", size.1)?;

        settings.set_boolean("is-maximized", self.is_maximized())?;

        Ok(())
    }

    fn load_window_size(&self) {
        let settings = &imp::QemuApplicationWindow::from_instance(self).settings;

        let width = settings.get_int("window-width");
        let height = settings.get_int("window-height");
        let is_maximized = settings.get_boolean("is-maximized");

        self.set_default_size(width, height);

        if is_maximized {
            self.maximize();
        }
    }
}
