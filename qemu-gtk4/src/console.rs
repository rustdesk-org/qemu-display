use glib::clone;
use glib::subclass::prelude::*;
use gtk::glib::translate::FromGlibPtrBorrow;
use gtk::prelude::*;
use gtk::{gdk, glib, CompositeTemplate};
use log::debug;
use once_cell::sync::OnceCell;
use std::cell::Cell;

use keycodemap::*;
use qemu_display_listener::{Console, ConsoleEvent as Event, MouseButton};

mod imp {
    use super::*;
    use gtk::subclass::prelude::*;

    #[derive(CompositeTemplate, Default)]
    #[template(resource = "/org/qemu/gtk4/console.ui")]
    pub struct QemuConsole {
        #[template_child]
        pub area: TemplateChild<crate::console_area::QemuConsoleArea>,
        #[template_child]
        pub label: TemplateChild<gtk::Label>,
        pub console: OnceCell<Console>,
        pub wait_rendering: Cell<usize>,
        pub shortcuts_inhibited_id: Cell<Option<glib::SignalHandlerId>>,
        pub ungrab_shortcut: OnceCell<gtk::ShortcutTrigger>,
        pub key_controller: OnceCell<gtk::EventControllerKey>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for QemuConsole {
        const NAME: &'static str = "QemuConsole";
        type Type = super::QemuConsole;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for QemuConsole {
        fn constructed(&self, obj: &Self::Type) {
            self.parent_constructed(obj);

            // TODO: implement a custom trigger with only modifiers, ala spice-gtk?
            let ungrab = gtk::ShortcutTrigger::parse_string("<ctrl><alt>g").unwrap();
            self.ungrab_shortcut.set(ungrab).unwrap();

            let ec = gtk::EventControllerKey::new();
            ec.set_propagation_phase(gtk::PropagationPhase::Capture);
            self.area.add_controller(&ec);
            ec.connect_key_pressed(
                clone!(@weak obj => @default-panic, move |_, _keyval, keycode, _state| {
                    let c = obj.qemu_console();
                    if let Some(qnum) = KEYMAP_XORGEVDEV2QNUM.get(keycode as usize) {
                        let _ = c.keyboard.press(*qnum as u32);
                    }
                    glib::signal::Inhibit(true)
                }),
            );
            ec.connect_key_released(clone!(@weak obj => move |_, _keyval, keycode, _state| {
                let c = obj.qemu_console();
                if let Some(qnum) = KEYMAP_XORGEVDEV2QNUM.get(keycode as usize) {
                    let _ = c.keyboard.release(*qnum as u32);
                }
            }));
            self.key_controller.set(ec).unwrap();

            let ec = gtk::EventControllerMotion::new();
            self.area.add_controller(&ec);
            ec.connect_motion(clone!(@weak obj => move |_, x, y| {
                let priv_ = imp::QemuConsole::from_instance(&obj);
                let c = obj.qemu_console();
                if let Ok(abs) = c.mouse.is_absolute() {
                    if abs {
                        priv_.motion(x, y);
                    } else {
                        dbg!()
                    }
                }
            }));

            let ec = gtk::GestureClick::new();
            ec.set_button(0);
            self.area.add_controller(&ec);
            ec.connect_pressed(clone!(@weak obj => @default-panic, move |gesture, _n_press, x, y| {
                let priv_ = imp::QemuConsole::from_instance(&obj);
                let c = obj.qemu_console();
                let button = from_gdk_button(gesture.get_current_button());
                priv_.motion(x, y);
                let _ = c.mouse.press(button);

                if let Some(toplevel) = priv_.get_toplevel() {
                    if !toplevel.get_property_shortcuts_inhibited() {
                        toplevel.inhibit_system_shortcuts::<gdk::ButtonEvent>(None);

                        let ec = gtk::EventControllerKey::new();
                        ec.set_propagation_phase(gtk::PropagationPhase::Capture);
                        ec.connect_key_pressed(clone!(@weak obj, @weak toplevel => @default-panic, move |ec, keyval, keycode, state| {
                            let priv_ = imp::QemuConsole::from_instance(&obj);
                            if let Some(ref e) = ec.get_current_event() {
                                if priv_.ungrab_shortcut.get().unwrap().trigger(e, false) == gdk::KeyMatch::Exact {
                                    //widget.remove_controller(ec); here crashes badly
                                    glib::idle_add_local(clone!(@weak ec, @weak toplevel => @default-panic, move || {
                                        if let Some(widget) = ec.get_widget() {
                                            widget.remove_controller(&ec);
                                        }
                                        toplevel.restore_system_shortcuts();
                                        glib::Continue(false)
                                    }));
                                } else {
                                    priv_.key_controller.get().unwrap().emit_by_name("key-pressed", &[&*keyval, &keycode, &state]).unwrap();
                                }
                            }

                            glib::signal::Inhibit(true)
                        }));
                        ec.connect_key_released(clone!(@weak obj => @default-panic, move |_ec, keyval, keycode, state| {
                            let priv_ = imp::QemuConsole::from_instance(&obj);
                            priv_.key_controller.get().unwrap().emit_by_name("key-released", &[&*keyval, &keycode, &state]).unwrap();
                        }));
                        if let Some(root) = priv_.area.get_root() {
                            root.add_controller(&ec);
                        }

                        let id = toplevel.connect_property_shortcuts_inhibited_notify(clone!(@weak obj => @default-panic, move |toplevel| {
                            let inhibited = toplevel.get_property_shortcuts_inhibited();
                            debug!("shortcuts-inhibited: {}", inhibited);
                            if !inhibited {
                                let priv_ = imp::QemuConsole::from_instance(&obj);
                                let id = priv_.shortcuts_inhibited_id.take();
                                toplevel.disconnect(id.unwrap());
                            }
                        }));
                        priv_.shortcuts_inhibited_id.set(Some(id));
                    }
                }

                priv_.area.grab_focus();
            }));
            ec.connect_released(clone!(@weak obj => move |gesture, _n_press, x, y| {
                let priv_ = imp::QemuConsole::from_instance(&obj);
                let c = obj.qemu_console();
                let button = from_gdk_button(gesture.get_current_button());

                priv_.motion(x, y);
                let _ = c.mouse.release(button);
            }));

            let ec = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
            self.area.add_controller(&ec);
            ec.connect_scroll(clone!(@weak obj => @default-panic, move |_, _dx, dy| {
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

            self.area
                .connect_create_context(clone!(@weak obj => @default-panic, move |_| {
                    // can't connect-after create-context yet, so idle it
                    glib::idle_add_local(clone!(@weak ec => @default-panic, move || {
                        let priv_ = imp::QemuConsole::from_instance(&obj);
                        priv_.attach_qemu_console(&obj);
                        glib::Continue(false)
                    }));
                    None
                }));

            unsafe {
                self.area.connect_notify_unsafe(
                    Some("resize-hack"),
                    clone!(@weak obj => move |_, _| {
                        let priv_ = imp::QemuConsole::from_instance(&obj);
                        let alloc = priv_.area.get_allocation();
                        if let Err(e) = obj.qemu_console().proxy.set_ui_info(0, 0, 0, 0, alloc.width as u32, alloc.height as u32) {
                            eprintln!("Failed to SetUIInfo: {}", e);
                        }
                    }),
                );
            }
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

    impl WidgetImpl for QemuConsole {}

    impl QemuConsole {
        fn get_toplevel(&self) -> Option<gdk::Toplevel> {
            self.area
                .get_root()
                .and_then(|r| r.get_native())
                .and_then(|n| n.get_surface())
                .and_then(|s| s.downcast::<gdk::Toplevel>().ok())
        }

        fn motion(&self, x: f64, y: f64) {
            if let Some((x, y)) = self.area.transform_input(x, y) {
                let c = self.console.get().unwrap();
                let _ = c.mouse.set_abs_position(x, y);
            }
        }

        pub(crate) fn attach_qemu_console(&self, obj: &super::QemuConsole) {
            let console = match self.console.get() {
                Some(console) => console,
                None => return,
            };
            if !obj.get_realized() {
                return;
            }

            let (rx, wait_tx) = console
                .glib_listen()
                .expect("Failed to listen to the console");
            self.area
                .connect_render(clone!(@weak obj => @default-panic, move |_, _| {
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
                clone!(@weak obj => @default-panic, move |t| {
                    let priv_ = imp::QemuConsole::from_instance(&obj);
                    debug!("Console event: {:?}", t);
                    match t {
                        Event::Scanout(s) => {
                            priv_.area.set_scanout(s);
                            priv_.area.queue_render();
                        }
                        Event::Update(u) => {
                            priv_.area.update(u);
                            priv_.area.queue_render();
                        }
                        Event::ScanoutDMABUF(s) => {
                            priv_.label.set_label(&format!("{:?}", s));
                            priv_.area.set_scanout_dmabuf(s);
                        }
                        Event::UpdateDMABUF { .. } => {
                            priv_.wait_rendering.set(priv_.wait_rendering.get() + 1);
                            // we don't simply queue_render, as we want a copy immediately
                            priv_.area.make_current();
                            priv_.area.attach_buffers();
                            let _ = unsafe {
                                glib::Object::from_glib_borrow(priv_.area.as_ptr() as *mut glib::gobject_ffi::GObject)
                                    .emit_by_name("render", &[&priv_.area.get_context().as_ref()])
                                    .unwrap()
                            };
                            priv_.area.queue_draw();
                        }
                        Event::Disconnected => {
                            priv_.label.set_label("Console disconnected!");
                        }
                        Event::CursorDefine { width, height, hot_x, hot_y, data }=> {
                            let scale = priv_.area.get_scale_factor();
                            let pb = gdk::gdk_pixbuf::Pixbuf::from_mut_slice(data, gdk::gdk_pixbuf::Colorspace::Rgb, true, 8, width, height, width * 4);
                            let pb = pb.scale_simple(width * scale, height * scale, gdk::gdk_pixbuf::InterpType::Bilinear).unwrap();
                            let tex = gdk::Texture::new_for_pixbuf(&pb);
                            let cur = gdk::Cursor::from_texture(&tex, hot_x * scale, hot_y * scale, None);
                            priv_.area.cursor_define(cur);
                        }
                        Event::MouseSet(m) => {
                            priv_.area.mouse_set(m);
                            let c = obj.qemu_console();
                            if let Ok(abs) = c.mouse.is_absolute() {
                                priv_.area.set_cursor_abs(abs);
                            }
                            priv_.area.queue_render();
                        }
                    }
                    Continue(true)
                }),
            );
        }
    }
}

glib::wrapper! {
    pub struct QemuConsole(ObjectSubclass<imp::QemuConsole>) @extends gtk::Widget;
}

impl QemuConsole {
    pub fn set_qemu_console(&self, console: Console) {
        let priv_ = imp::QemuConsole::from_instance(self);
        priv_.console.set(console).unwrap();
        priv_.attach_qemu_console(self);
    }

    fn qemu_console(&self) -> &Console {
        let priv_ = imp::QemuConsole::from_instance(self);
        priv_.console.get().expect("Console is not yet set!")
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
