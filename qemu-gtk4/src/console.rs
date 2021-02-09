use glib::clone;
use glib::subclass::prelude::*;
use gtk::prelude::*;
use gtk::subclass::widget::WidgetImplExt;
use gtk::{glib, CompositeTemplate};
use once_cell::sync::OnceCell;

use qemu_display_listener::{Console, Event, MouseButton};

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

            let ec = gtk::EventControllerKey::new();
            ec.set_propagation_phase(gtk::PropagationPhase::Capture);
            self.area.add_controller(&ec);
            ec.connect_key_pressed(clone!(@weak obj => move |_, _keyval, keycode, _state| {
                let c = obj.qemu_console();
                let _ = c.keyboard.press(keycode);
                glib::signal::Inhibit(true)
            }));
            ec.connect_key_released(clone!(@weak obj => move |_, _keyval, keycode, _state| {
                let c = obj.qemu_console();
                let _ = c.keyboard.release(keycode);
            }));

            let ec = gtk::EventControllerMotion::new();
            self.area.add_controller(&ec);
            ec.connect_motion(clone!(@weak obj => move |_, x, y| {
                obj.motion(x, y);
            }));

            let ec = gtk::GestureClick::new();
            ec.set_button(0);
            self.area.add_controller(&ec);
            ec.connect_pressed(clone!(@weak obj => move |gesture, _n_press, x, y| {
                let c = obj.qemu_console();
                let button = from_gdk_button(gesture.get_current_button());
                obj.motion(x, y);
                let _ = c.mouse.press(button);
            }));
            ec.connect_released(clone!(@weak obj => move |gesture, _n_press, x, y| {
                let c = obj.qemu_console();
                let button = from_gdk_button(gesture.get_current_button());
                obj.motion(x, y);
                let _ = c.mouse.release(button);
            }));

            self.area.set_sensitive(true);
            self.area.set_focusable(true);
            self.area.set_focus_on_click(true);
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
                    Event::Disconnected => {
                        con.label.set_label("Console disconnected!");
                    }
                    _ => ()
                }
                Continue(true)
            }),
        );
        priv_.console.set(console).unwrap();
    }

    fn qemu_console(&self) -> &Console {
        let priv_ = imp::QemuConsole::from_instance(self);
        priv_.console.get().expect("Console is not yet set!")
    }

    fn motion(&self, x: f64, y: f64) {
        let priv_ = imp::QemuConsole::from_instance(self);

        // FIXME: scaling, centering etc..
        let widget_w = self.get_width();
        let widget_h = self.get_height();
        let _widget_scale = self.get_scale_factor();

        let c = self.qemu_console();
        // FIXME: ideally, we would use ConsoleProxy cached properties instead
        let x = (x / widget_w as f64) * priv_.area.scanout_size().0 as f64;
        let y = (y / widget_h as f64) * priv_.area.scanout_size().1 as f64;
        let _ = c.mouse.set_abs_position(x as u32, y as u32);

        // FIXME: focus on click doesn't work
        priv_.area.grab_focus();
    }
}

fn from_gdk_button(button: u32) -> MouseButton {
    match button {
        1 => MouseButton::Left,
        2 => MouseButton::Middle,
        3 => MouseButton::Right,
        _ => MouseButton::Extra,
    }
}
