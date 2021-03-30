use std::cell::RefCell;
use std::ops::Drop;
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::mpsc::{Receiver, RecvError, SendError};
use std::sync::Arc;

use derivative::Derivative;
use zbus::{dbus_interface, export::zvariant::Fd};

use crate::EventSender;

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

#[derive(Debug, Copy, Clone)]
pub struct MouseSet {
    pub x: i32,
    pub y: i32,
    pub on: i32,
}

// TODO: replace events mpsc with async traits
#[derive(Debug)]
pub enum ConsoleEvent {
    Scanout(Scanout),
    Update(Update),
    ScanoutDMABUF(ScanoutDMABUF),
    UpdateDMABUF {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    },
    MouseSet(MouseSet),
    CursorDefine {
        width: i32,
        height: i32,
        hot_x: i32,
        hot_y: i32,
        data: Vec<u8>,
    },
    Disconnected,
}

#[derive(Debug)]
pub(crate) struct ConsoleListener<E: EventSender<Event = ConsoleEvent>> {
    tx: E,
    wait_rx: Receiver<()>,
    err: Arc<RefCell<Option<SendError<ConsoleEvent>>>>,
}

#[dbus_interface(name = "org.qemu.Display1.Listener")]
impl<E: 'static + EventSender<Event = ConsoleEvent>> ConsoleListener<E> {
    fn scanout(
        &mut self,
        width: u32,
        height: u32,
        stride: u32,
        format: u32,
        data: serde_bytes::ByteBuf,
    ) {
        self.send(ConsoleEvent::Scanout(Scanout {
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
        self.send(ConsoleEvent::Update(Update {
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
        self.send(ConsoleEvent::ScanoutDMABUF(ScanoutDMABUF {
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
        self.send(ConsoleEvent::UpdateDMABUF { x, y, w, h });
        if let Err(e) = self.wait() {
            eprintln!("update returned error: {}", e)
        }
    }

    fn mouse_set(&mut self, x: i32, y: i32, on: i32) {
        self.send(ConsoleEvent::MouseSet(MouseSet { x, y, on }))
    }

    fn cursor_define(&mut self, width: i32, height: i32, hot_x: i32, hot_y: i32, data: Vec<u8>) {
        self.send(ConsoleEvent::CursorDefine {
            width,
            height,
            hot_x,
            hot_y,
            data,
        })
    }
}

impl<E: EventSender<Event = ConsoleEvent>> ConsoleListener<E> {
    pub(crate) fn new(tx: E, wait_rx: Receiver<()>) -> Self {
        let err = Arc::new(RefCell::new(None));
        ConsoleListener { tx, wait_rx, err }
    }

    fn send(&mut self, event: ConsoleEvent) {
        if let Err(e) = self.tx.send_event(event) {
            *self.err.borrow_mut() = Some(e);
        }
    }

    fn wait(&mut self) -> Result<(), RecvError> {
        self.wait_rx.recv()
    }

    pub fn err(&self) -> Arc<RefCell<Option<SendError<ConsoleEvent>>>> {
        self.err.clone()
    }
}

impl<E: EventSender<Event = ConsoleEvent>> Drop for ConsoleListener<E> {
    fn drop(&mut self) {
        self.send(ConsoleEvent::Disconnected)
    }
}
