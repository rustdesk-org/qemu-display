use std::cell::RefCell;
use std::ops::Drop;
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::mpsc::{Receiver, RecvError, SendError, Sender};
use std::sync::Arc;

use derivative::Derivative;
use zbus::{dbus_interface, export::zvariant::Fd};

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Scanout {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: u32,
    #[derivative(Debug = "ignore")]
    pub data: Vec<u8>,
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Update {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub stride: u32,
    pub format: u32,
    #[derivative(Debug = "ignore")]
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub struct ScanoutDMABUF {
    pub fd: RawFd,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub fourcc: u32,
    pub modifier: u64,
    pub y0_top: bool,
}

impl Drop for ScanoutDMABUF {
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
    Scanout(Scanout),
    Update(Update),
    ScanoutDMABUF(ScanoutDMABUF),
    UpdateDMABUF {
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
    wait_rx: Receiver<()>,
    err: Arc<RefCell<Option<SendError<Event>>>>,
}

#[dbus_interface(name = "org.qemu.Display1.Listener")]
impl<E: 'static + EventSender> Listener<E> {
    fn scanout(
        &mut self,
        width: u32,
        height: u32,
        stride: u32,
        format: u32,
        data: serde_bytes::ByteBuf,
    ) {
        self.send(Event::Scanout(Scanout {
            width,
            height,
            stride,
            format,
            data: data.into_vec(),
        }))
    }

    fn update(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        stride: u32,
        format: u32,
        data: serde_bytes::ByteBuf,
    ) {
        self.send(Event::Update(Update {
            x,
            y,
            w,
            h,
            stride,
            format,
            data: data.into_vec(),
        }))
    }

    #[dbus_interface(name = "ScanoutDMABUF")]
    fn scanout_dmabuf(
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
        self.send(Event::ScanoutDMABUF(ScanoutDMABUF {
            fd,
            width,
            height,
            stride,
            fourcc,
            modifier,
            y0_top,
        }))
    }

    #[dbus_interface(name = "UpdateDMABUF")]
    fn update_dmabuf(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.send(Event::UpdateDMABUF { x, y, w, h });
        if let Err(e) = self.wait() {
            eprintln!("update returned error: {}", e)
        }
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
    pub(crate) fn new(tx: E, wait_rx: Receiver<()>) -> Self {
        let err = Arc::new(RefCell::new(None));
        Listener { tx, wait_rx, err }
    }

    fn send(&mut self, event: Event) {
        if let Err(e) = self.tx.send_event(event) {
            *self.err.borrow_mut() = Some(e);
        }
    }

    fn wait(&mut self) -> Result<(), RecvError> {
        self.wait_rx.recv()
    }

    pub fn err(&self) -> Arc<RefCell<Option<SendError<Event>>>> {
        self.err.clone()
    }
}

impl<E: EventSender> Drop for Listener<E> {
    fn drop(&mut self) {
        self.send(Event::Disconnected)
    }
}
