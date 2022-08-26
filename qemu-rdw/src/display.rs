use futures_util::StreamExt;
use glib::{clone, subclass::prelude::*, MainContext};
use gtk::glib;
use keycodemap::KEYMAP_XORGEVDEV2QNUM;
use once_cell::sync::OnceCell;
use qemu_display::{Console, ConsoleListenerHandler};
use rdw::{gtk, DisplayExt};
#[cfg(unix)]
use std::os::unix::io::IntoRawFd;

mod imp {
    use super::*;
    use gtk::subclass::prelude::*;

    #[repr(C)]
    pub struct RdwDisplayQemuClass {
        pub parent_class: rdw::RdwDisplayClass,
    }

    unsafe impl ClassStruct for RdwDisplayQemuClass {
        type Type = Display;
    }

    #[repr(C)]
    pub struct RdwDisplayQemu {
        parent: rdw::RdwDisplay,
    }

    impl std::fmt::Debug for RdwDisplayQemu {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.debug_struct("RdwDisplayQemu")
                .field("parent", &self.parent)
                .finish()
        }
    }

    unsafe impl InstanceStruct for RdwDisplayQemu {
        type Type = Display;
    }

    #[derive(Debug, Default)]
    pub struct Display {
        pub(crate) console: OnceCell<Console>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Display {
        const NAME: &'static str = "RdwDisplayQemu";
        type Type = super::Display;
        type ParentType = rdw::Display;
        type Class = RdwDisplayQemuClass;
        type Instance = RdwDisplayQemu;
    }

    impl ObjectImpl for Display {
        fn constructed(&self, obj: &Self::Type) {
            self.parent_constructed(obj);

            obj.set_mouse_absolute(true);

            obj.connect_key_event(clone!(@weak obj => move |_, keyval, keycode, event| {
                log::debug!("key-event: {:?}", (keyval, keycode, event));
                if let Some(qnum) = KEYMAP_XORGEVDEV2QNUM.get(keycode as usize) {
                    MainContext::default().spawn_local(clone!(@weak obj => async move {
                        if event.contains(rdw::KeyEvent::PRESS) {
                            let _ = obj.console().keyboard.press(*qnum as u32).await;
                        }
                        if event.contains(rdw::KeyEvent::RELEASE) {
                            let _ = obj.console().keyboard.release(*qnum as u32).await;
                        }
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
                use qemu_display::MouseButton;

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

    impl WidgetImpl for Display {
        fn realize(&self, widget: &Self::Type) {
            self.parent_realize(widget);

            MainContext::default().spawn_local(clone!(@weak widget => async move {
                let self_ = Self::from_instance(&widget);
                let console = self_.console.get().unwrap();
                // we have to use a channel, because widget is not Send..
                let (sender, mut receiver) = futures::channel::mpsc::unbounded();
                console.register_listener(ConsoleHandler { sender }).await.unwrap();
                MainContext::default().spawn_local(clone!(@weak widget => async move {
                    while let Some(e) = receiver.next().await {
                        use ConsoleEvent::*;
                        match e {
                            Scanout(s) => {
                                if s.format != 0x20020888 {
                                    log::warn!("Format not yet supported: {:X}", s.format);
                                    continue;
                                }
                                widget.set_display_size(Some((s.width as _, s.height as _)));
                                widget.update_area(0, 0, s.width as _, s.height as _, s.stride as _, &s.data);
                            }
                            Update(u) => {
                                if u.format != 0x20020888 {
                                    log::warn!("Format not yet supported: {:X}", u.format);
                                    continue;
                                }
                                widget.update_area(u.x as _, u.y as _, u.w as _, u.h as _, u.stride as _, &u.data);
                            }
                            #[cfg(windows)]
                            ScanoutDMABUF(_) => {
                                unimplemented!()
                            }
                            #[cfg(unix)]
                            ScanoutDMABUF(s) => {
                                widget.set_display_size(Some((s.width as _, s.height as _)));
                                widget.set_dmabuf_scanout(rdw::RdwDmabufScanout {
                                    width: s.width,
                                    height: s.height,
                                    stride: s.stride,
                                    fourcc: s.fourcc,
                                    y0_top: s.y0_top,
                                    modifier: s.modifier,
                                    fd: s.into_raw_fd(),
                                });
                            }
                            UpdateDMABUF { wait_tx, .. } => {
                                widget.render();
                                let _ = wait_tx.send(());
                            }
                            Disconnected => {
                                log::warn!("Console disconnected");
                            }
                            CursorDefine(c) => {
                                let cursor = rdw::Display::make_cursor(
                                    &c.data,
                                    c.width,
                                    c.height,
                                    c.hot_x,
                                    c.hot_y,
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
                    }
                }));
                let mut abs_changed = console.mouse.receive_is_absolute_changed().await;
                MainContext::default().spawn_local(clone!(@weak widget => async move {
                    while let Some(abs) = abs_changed.next().await {
                        if let Ok(abs) = abs.get().await {
                            widget.set_mouse_absolute(abs);
                        }
                    }
                }));
            }));
        }
    }

    impl rdw::DisplayImpl for Display {}
}

glib::wrapper! {
    pub struct Display(ObjectSubclass<imp::Display>) @extends rdw::Display, gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Display {
    pub fn new(console: Console) -> Self {
        let obj = glib::Object::new::<Self>(&[]).unwrap();
        let self_ = imp::Display::from_instance(&obj);
        self_.console.set(console).unwrap();
        obj
    }

    pub(crate) fn console(&self) -> &Console {
        let self_ = imp::Display::from_instance(self);
        self_.console.get().unwrap()
    }
}

#[derive(Debug)]
enum ConsoleEvent {
    Scanout(qemu_display::Scanout),
    Update(qemu_display::Update),
    ScanoutDMABUF(qemu_display::ScanoutDMABUF),
    UpdateDMABUF {
        _update: qemu_display::UpdateDMABUF,
        wait_tx: futures::channel::oneshot::Sender<()>,
    },
    MouseSet(qemu_display::MouseSet),
    CursorDefine(qemu_display::Cursor),
    Disconnected,
}

struct ConsoleHandler {
    sender: futures::channel::mpsc::UnboundedSender<ConsoleEvent>,
}

impl ConsoleHandler {
    fn send(&self, event: ConsoleEvent) {
        if let Err(e) = self.sender.unbounded_send(event) {
            log::warn!("failed to send console event: {}", e);
        }
    }
}

#[async_trait::async_trait]
impl ConsoleListenerHandler for ConsoleHandler {
    async fn scanout(&mut self, scanout: qemu_display::Scanout) {
        self.send(ConsoleEvent::Scanout(scanout));
    }

    async fn update(&mut self, update: qemu_display::Update) {
        self.send(ConsoleEvent::Update(update));
    }

    async fn scanout_dmabuf(&mut self, scanout: qemu_display::ScanoutDMABUF) {
        self.send(ConsoleEvent::ScanoutDMABUF(scanout));
    }

    async fn update_dmabuf(&mut self, _update: qemu_display::UpdateDMABUF) {
        let (wait_tx, wait_rx) = futures::channel::oneshot::channel();
        self.send(ConsoleEvent::UpdateDMABUF { _update, wait_tx });
        if let Err(e) = wait_rx.await {
            log::warn!("wait update dmabuf failed: {}", e);
        }
    }

    async fn mouse_set(&mut self, set: qemu_display::MouseSet) {
        self.send(ConsoleEvent::MouseSet(set));
    }

    async fn cursor_define(&mut self, cursor: qemu_display::Cursor) {
        self.send(ConsoleEvent::CursorDefine(cursor));
    }

    fn disconnected(&mut self) {
        self.send(ConsoleEvent::Disconnected);
    }
}

fn from_gdk_button(button: u32) -> qemu_display::MouseButton {
    use qemu_display::MouseButton::*;

    match button {
        1 => Left,
        2 => Middle,
        3 => Right,
        _ => Extra,
    }
}
