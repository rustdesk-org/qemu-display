use glib::{clone, subclass::prelude::*, translate::*, MainContext};
use gtk::{glib, prelude::*};
use once_cell::sync::OnceCell;

use keycodemap::KEYMAP_XORGEVDEV2QNUM;
use qemu_display_listener::Console;
use rdw::DisplayExt;

mod imp {
    use super::*;
    use gtk::subclass::prelude::*;
    use std::os::unix::io::IntoRawFd;

    #[repr(C)]
    pub struct RdwDisplayQemuClass {
        pub parent_class: rdw::imp::RdwDisplayClass,
    }

    unsafe impl ClassStruct for RdwDisplayQemuClass {
        type Type = DisplayQemu;
    }

    #[repr(C)]
    pub struct RdwDisplayQemu {
        parent: rdw::imp::RdwDisplay,
    }

    impl std::fmt::Debug for RdwDisplayQemu {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.debug_struct("RdwDisplayQemu")
                .field("parent", &self.parent)
                .finish()
        }
    }

    unsafe impl InstanceStruct for RdwDisplayQemu {
        type Type = DisplayQemu;
    }

    #[derive(Debug, Default)]
    pub struct DisplayQemu {
        pub(crate) console: OnceCell<Console>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DisplayQemu {
        const NAME: &'static str = "RdwDisplayQemu";
        type Type = super::DisplayQemu;
        type ParentType = rdw::Display;
        type Class = RdwDisplayQemuClass;
        type Instance = RdwDisplayQemu;
    }

    impl ObjectImpl for DisplayQemu {
        fn constructed(&self, obj: &Self::Type) {
            self.parent_constructed(obj);

            obj.set_mouse_absolute(true);

            obj.connect_key_press(clone!(@weak obj => move |_, keyval, keycode| {
                log::debug!("key-press: {:?}", (keyval, keycode));
                if let Some(qnum) = KEYMAP_XORGEVDEV2QNUM.get(keycode as usize) {
                    MainContext::default().spawn_local(clone!(@weak obj => async move {
                        let _ = obj.console().keyboard.press(*qnum as u32).await;
                    }));
                }
            }));

            obj.connect_key_release(clone!(@weak obj => move |_, keyval, keycode| {
                log::debug!("key-release: {:?}", (keyval, keycode));
                if let Some(qnum) = KEYMAP_XORGEVDEV2QNUM.get(keycode as usize) {
                    MainContext::default().spawn_local(clone!(@weak obj => async move {
                        let _ = obj.console().keyboard.release(*qnum as u32).await;
                    }));
                }
            }));

            obj.connect_motion(clone!(@weak obj => move |_, x, y| {
                log::debug!("motion: {:?}", (x, y));
                MainContext::default().spawn_local(clone!(@weak obj => async move {
                    let _ = obj.console().mouse.set_abs_position(x as _, y as _).await;
                }));
            }));

            obj.connect_motion_relative(clone!(@weak obj => move |_, dx, dy| {
                log::debug!("motion-relative: {:?}", (dx, dy));
                MainContext::default().spawn_local(clone!(@weak obj => async move {
                    let _ = obj.console().mouse.rel_motion(dx as _, dy as _).await;
                }));
            }));

            obj.connect_mouse_press(clone!(@weak obj => move |_, button| {
                log::debug!("mouse-press: {:?}", button);
                MainContext::default().spawn_local(clone!(@weak obj => async move {
                    let button = from_gdk_button(button);
                    let _ = obj.console().mouse.press(button).await;
                }));
            }));

            obj.connect_mouse_release(clone!(@weak obj => move |_, button| {
                log::debug!("mouse-release: {:?}", button);
                MainContext::default().spawn_local(clone!(@weak obj => async move {
                    let button = from_gdk_button(button);
                    let _ = obj.console().mouse.release(button).await;
                }));
            }));

            obj.connect_scroll_discrete(clone!(@weak obj => move |_, scroll| {
                use qemu_display_listener::MouseButton;

                log::debug!("scroll-discrete: {:?}", scroll);

                let button = match scroll {
                    rdw::Scroll::Up => MouseButton::WheelUp,
                    rdw::Scroll::Down => MouseButton::WheelDown,
                    _ => {
                        log::warn!("not yet implemented");
                        return;
                    }
                };
                MainContext::default().spawn_local(clone!(@weak obj => async move {
                    let _ = obj.console().mouse.press(button).await;
                    let _ = obj.console().mouse.release(button).await;
                }));
            }));

            obj.connect_resize_request(clone!(@weak obj => move |_, width, height, wmm, hmm| {
                log::debug!("resize-request: {:?}", (width, height, wmm, hmm));
                MainContext::default().spawn_local(clone!(@weak obj => async move {
                    let _ = obj.console().proxy.set_ui_info(wmm as _, hmm as _, 0, 0, width, height).await;
                }));
            }));
        }
    }

    impl WidgetImpl for DisplayQemu {
        fn realize(&self, widget: &Self::Type) {
            self.parent_realize(widget);

            MainContext::default().spawn_local(clone!(@weak widget => async move {
                let self_ = Self::from_instance(&widget);
                let console = self_.console.get().unwrap();
                let (rx, wait_tx) = console
                    .glib_listen()
                    .await
                    .expect("Failed to listen to the console");
                rx.attach(
                    None,
                    clone!(@weak widget => @default-panic, move |evt| {
                        use qemu_display_listener::ConsoleEvent::*;

                        log::debug!("Console event: {:?}", evt);
                        match evt {
                            Scanout(s) => {
                                if s.format != 0x20020888 {
                                    log::warn!("Format not yet supported: {:X}", s.format);
                                    return Continue(true);
                                }
                                widget.set_display_size(Some((s.width as _, s.height as _)));
                                widget.update_area(0, 0, s.width as _, s.height as _, s.stride as _, &s.data);
                            }
                            Update(u) => {
                                if u.format != 0x20020888 {
                                    log::warn!("Format not yet supported: {:X}", u.format);
                                    return Continue(true);
                                }
                                widget.update_area(u.x as _, u.y as _, u.w as _, u.h as _, u.stride as _, &u.data);
                            }
                            ScanoutDMABUF(s) => {
                                widget.set_display_size(Some((s.width as _, s.height as _)));
                                widget.set_dmabuf_scanout(rdw::DmabufScanout {
                                    width: s.width,
                                    height: s.height,
                                    stride: s.stride,
                                    fourcc: s.fourcc,
                                    y0_top: s.y0_top,
                                    modifier: s.modifier,
                                    fd: s.into_raw_fd(),
                                });
                            }
                            UpdateDMABUF { .. } => {
                                widget.render();
                                let _ = wait_tx.send(());
                            }
                            Disconnected => {
                            }
                            CursorDefine { width, height, hot_x, hot_y, data }=> {
                                let cursor = rdw::Display::make_cursor(
                                    &data,
                                    width,
                                    height,
                                    hot_x,
                                    hot_y,
                                    1,
                                );
                                widget.define_cursor(Some(cursor));
                            }
                            MouseSet(m) => {
                                if m.on != 0 {
                                    widget.set_cursor_position(Some((m.x as _, m.y as _)));
                                } else {
                                    widget.set_cursor_position(None);
                                }
                            }
                        }
                        Continue(true)
                    })
                );
            }));
        }
    }

    impl rdw::DisplayImpl for DisplayQemu {}

    impl DisplayQemu {
        pub(crate) fn set_console(&self, console: Console) {
            self.console.set(console).unwrap();
        }
    }
}

glib::wrapper! {
    pub struct DisplayQemu(ObjectSubclass<imp::DisplayQemu>) @extends rdw::Display, gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl DisplayQemu {
    pub fn new(console: Console) -> Self {
        let obj = glib::Object::new::<Self>(&[]).unwrap();
        let self_ = imp::DisplayQemu::from_instance(&obj);
        self_.set_console(console);
        obj
    }

    pub(crate) fn console(&self) -> &Console {
        let self_ = imp::DisplayQemu::from_instance(self);
        self_.console.get().unwrap()
    }
}

fn from_gdk_button(button: u32) -> qemu_display_listener::MouseButton {
    use qemu_display_listener::MouseButton::*;

    match button {
        1 => Left,
        2 => Middle,
        3 => Right,
        _ => Extra,
    }
}
