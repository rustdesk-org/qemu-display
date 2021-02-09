use std::cell::Cell;
use glib::subclass::prelude::*;
use glib::clone;
use gtk::prelude::*;
use gtk::{glib, graphene, gdk};

use qemu_display_listener::Scanout;

mod imp {
    use super::*;
    use glib::subclass;
    use gtk::subclass::prelude::*;

    pub struct QemuConsoleArea {
        pub scanout: Cell<Option<Scanout>>,
    }

    impl ObjectSubclass for QemuConsoleArea {
        const NAME: &'static str = "QemuConsoleArea";
        type Type = super::QemuConsoleArea;
        type ParentType = gtk::Widget;
        type Interfaces = ();
        type Instance = subclass::simple::InstanceStruct<Self>;
        type Class = subclass::simple::ClassStruct<Self>;

        glib::object_subclass!();

        fn new() -> Self {
            Self {
                scanout: Cell::new(None),
            }
        }
    }

    impl ObjectImpl for QemuConsoleArea {
        fn constructed(&self, obj: &Self::Type) {
            self.parent_constructed(obj);

            let ec = gtk::EventControllerLegacy::new();
            // XXX: where are the key events?
            // ec.set_propagation_phase(gtk::PropagationPhase::Bubble);
            obj.add_controller(&ec);
            ec.connect_event(clone!(@weak obj => move |_, e| {
                dbg!(e);
                true
            }));
            obj.set_focusable(true);
            obj.set_focus_on_click(true);
        }
    }

    impl WidgetImpl for QemuConsoleArea {
        fn snapshot(&self, widget: &Self::Type, snapshot: &gtk::Snapshot) {
            let (width, height) = (widget.get_width() as f32, widget.get_height() as f32);
            let whole = &graphene::Rect::new(0_f32, 0_f32, width, height);
            // TODO: make this a CSS style?
            snapshot.append_color(&gdk::RGBA::black(), whole);
            //snapshot.append_texture(priv_.texture, whole);
        }
    }
}

glib::wrapper! {
    pub struct QemuConsoleArea(ObjectSubclass<imp::QemuConsoleArea>) @extends gtk::Widget;
}

impl QemuConsoleArea {
    pub fn set_scanout(&self, s: Scanout) {
        let priv_ = imp::QemuConsoleArea::from_instance(self);
        priv_.scanout.replace(Some(s));
    }
}
