use glib::subclass::prelude::*;
use glib::clone;
use gtk::prelude::*;
use gtk::subclass::widget::WidgetImplExt;
use gtk::{glib, CompositeTemplate};
use once_cell::sync::OnceCell;

use qemu_display_listener::{Console, Event};

mod imp {
    use super::*;
    use glib::subclass;
    use gtk::subclass::prelude::*;

    #[derive(Debug, CompositeTemplate, Default)]
    #[template(resource = "/org/qemu/gtk4/console.ui")]
    pub struct QemuConsole {
        #[template_child]
        pub area: TemplateChild<crate::console_area::QemuConsoleArea>,
        #[template_child]
        pub label: TemplateChild<gtk::Label>,
        pub console: OnceCell<Console>,
    }

    impl ObjectSubclass for QemuConsole {
        const NAME: &'static str = "QemuConsole";
        type Type = super::QemuConsole;
        type ParentType = gtk::Widget;
        type Interfaces = ();
        type Instance = subclass::simple::InstanceStruct<Self>;
        type Class = subclass::simple::ClassStruct<Self>;

        glib::object_subclass!();

        fn new() -> Self {
            Self::default()
        }

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self::Type>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for QemuConsole {
        fn constructed(&self, obj: &Self::Type) {
            self.parent_constructed(obj);
        }

        // Needed for direct subclasses of GtkWidget;
        // Here you need to unparent all direct children
        // of your template.
        fn dispose(&self, obj: &Self::Type) {
            while let Some(child) = obj.get_first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for QemuConsole {
        fn size_allocate(&self, widget: &Self::Type, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(widget, width, height, baseline);
        }
    }
}

glib::wrapper! {
    pub struct QemuConsole(ObjectSubclass<imp::QemuConsole>) @extends gtk::Widget;
}

impl QemuConsole {
    pub fn set_qemu_console(&self, console: Console) {
        let priv_ = imp::QemuConsole::from_instance(self);
        let rx = console
            .glib_listen()
            .expect("Failed to listen to the console");
        rx.attach(
            None,
            clone!(@weak self as con => move |t| {
                let con = imp::QemuConsole::from_instance(&con);
                match t {
                    Event::Scanout(s) => {
                        con.label.set_label(&format!("{:?}", s));
                        con.area.set_scanout(s);
                    }
                    _ => ()
                }
                Continue(true)
            }),
        );
        priv_.console.set(console).unwrap();
    }
}
