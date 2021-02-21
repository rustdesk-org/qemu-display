use glib::clone;
use glib::subclass::prelude::*;
use gtk::prelude::*;
use gtk::subclass::widget::WidgetImplExt;
use gtk::{gdk, glib, CompositeTemplate};
use once_cell::sync::OnceCell;
use std::cell::Cell;

use keycodemap::*;
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
        pub wait_rendering: Cell<usize>,
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
                if let Some(qnum) = KEYMAP_XORGEVDEV2QNUM.get(keycode as usize) {
                    let _ = c.keyboard.press(*qnum as u32);
                }
                glib::signal::Inhibit(true)
            }));
            ec.connect_key_released(clone!(@weak obj => move |_, _keyval, keycode, _state| {
                let c = obj.qemu_console();
                if let Some(qnum) = KEYMAP_XORGEVDEV2QNUM.get(keycode as usize) {
                    let _ = c.keyboard.release(*qnum as u32);
                }
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

            let ec = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
            self.area.add_controller(&ec);
            ec.connect_scroll(clone!(@weak obj => move |_, _dx, dy| {
                let c = obj.qemu_console();

                let button = if dy >= 1.0 {
                    Some(MouseButton::WheelDown)
                } else if dy <= -1.0 {
                    Some(MouseButton::WheelUp)
                } else {
                    None
                };
                if let Some(button) = button {
                    let _ = c.mouse.press(button);
                    let _ = c.mouse.release(button);
                }
                glib::signal::Inhibit(true)
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
        let (rx, wait_tx) = console
            .glib_listen()
            .expect("Failed to listen to the console");
        priv_
            .area
            .connect_render(clone!(@weak self as obj => move |_, _| {
                let priv_ = imp::QemuConsole::from_instance(&obj);
                let wait_rendering = priv_.wait_rendering.get();
                if wait_rendering > 0 {
                    if let Err(e) = wait_tx.send(()) {
                        eprintln!("Failed to ack rendering: {}", e);
                    }
                    priv_.wait_rendering.set(wait_rendering - 1);
                }
                glib::signal::Inhibit(false)
            }));
        rx.attach(
            None,
            clone!(@weak self as con => move |t| {
                let priv_ = imp::QemuConsole::from_instance(&con);
                match t {
                    Event::Update { .. } => {
                        priv_.wait_rendering.set(priv_.wait_rendering.get() + 1);
                        priv_.area.queue_render();
                    }
                    Event::Scanout(s) => {
                        priv_.label.set_label(&format!("{:?}", s));
                        priv_.area.set_scanout(s);
                    }
                    Event::Disconnected => {
                        priv_.label.set_label("Console disconnected!");
                    }
                    Event::CursorDefine { width, height, hot_x, hot_y, data }=> {
                        let bytes = glib::Bytes::from(&data);
                        let tex = gdk::MemoryTexture::new(width, height, gdk::MemoryFormat::B8g8r8a8, &bytes, width as usize * 4);
                        let cur = gdk::Cursor::from_texture(&tex, hot_x, hot_y, None);
                        priv_.area.set_cursor(Some(&cur));
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

        if let Some((x, y)) = priv_.area.transform_input(x, y) {
            let c = self.qemu_console();
            let _ = c.mouse.set_abs_position(x, y);
        }

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
