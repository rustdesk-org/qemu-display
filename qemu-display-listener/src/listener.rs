use std::cell::RefCell;
use std::ops::Drop;
use std::os::unix::io::{AsRawFd, RawFd};
use std::rc::Rc;
use std::sync::mpsc::{SendError, Sender};

use zbus::{dbus_interface, export::zvariant::Fd};

#[derive(Debug)]
pub struct Scanout {
    pub fd: RawFd,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub fourcc: u32,
    pub modifier: u64,
    pub y0_top: bool,
}

impl Drop for Scanout {
    fn drop(&mut self) {
        if self.fd >= 0 {
            unsafe {
                libc::close(self.fd);
            }
        }
    }
}

// TODO: replace events mpsc with async traits
#[derive(Debug)]
pub enum Event {
    Switch {
        width: i32,
        height: i32,
    },
    Update {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    },
    MouseSet {
        x: i32,
        y: i32,
        on: i32,
    },
    CursorDefine {
        width: i32,
        height: i32,
        hot_x: i32,
        hot_y: i32,
        data: Vec<u8>,
    },
    Scanout(Scanout),
    Disconnected,
}

pub(crate) trait EventSender {
    fn send_event(&self, t: Event) -> Result<(), SendError<Event>>;
}

impl EventSender for Sender<Event> {
    fn send_event(&self, t: Event) -> Result<(), SendError<Event>> {
        self.send(t)
    }
}

#[cfg(feature = "glib")]
impl EventSender for glib::Sender<Event> {
    fn send_event(&self, t: Event) -> Result<(), SendError<Event>> {
        self.send(t)
    }
}

#[derive(Debug)]
pub(crate) struct Listener<E: EventSender> {
    tx: E,
    err: Rc<RefCell<Option<SendError<Event>>>>,
}

#[dbus_interface(name = "org.qemu.Display1.Listener")]
impl<E: 'static + EventSender> Listener<E> {
    fn switch(&mut self, width: i32, height: i32) {
        self.send(Event::Switch { width, height })
    }

    fn update(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.send(Event::Update { x, y, w, h })
    }

    fn scanout(
        &mut self,
        fd: Fd,
        width: u32,
        height: u32,
        stride: u32,
        fourcc: u32,
        modifier: u64,
        y0_top: bool,
    ) {
        let fd = unsafe { libc::dup(fd.as_raw_fd()) };
        self.send(Event::Scanout(Scanout {
            fd,
            width,
            height,
            stride,
            fourcc,
            modifier,
            y0_top,
        }))
    }

    fn mouse_set(&mut self, x: i32, y: i32, on: i32) {
        self.send(Event::MouseSet { x, y, on })
    }

    fn cursor_define(&mut self, width: i32, height: i32, hot_x: i32, hot_y: i32, data: Vec<u8>) {
        self.send(Event::CursorDefine {
            width,
            height,
            hot_x,
            hot_y,
            data,
        })
    }
}

impl<E: EventSender> Listener<E> {
    pub(crate) fn new(tx: E, err: Rc<RefCell<Option<SendError<Event>>>>) -> Self {
        Listener { tx, err }
    }

    fn send(&mut self, event: Event) {
        if let Err(e) = self.tx.send_event(event) {
            *self.err.borrow_mut() = Some(e);
        }
    }
}

impl<E: EventSender> Drop for Listener<E> {
    fn drop(&mut self) {
        self.send(Event::Disconnected)
    }
}
