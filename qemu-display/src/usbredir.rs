use std::{
    cell::RefCell,
    collections::HashMap,
    default::Default,
    io::{Read, Write},
    os::unix::{
        io::{AsRawFd, RawFd},
        net::UnixStream,
    },
    sync::{Arc, Mutex},
    thread::JoinHandle,
};

use usbredirhost::{
    rusb::{self, UsbContext},
    Device, DeviceHandler, LogLevel,
};

use crate::{Chardev, Error, Result};

#[derive(Debug)]
struct InnerHandler {
    device_fd: Option<zvariant::OwnedFd>,
    stream: UnixStream,
    stream_thread: JoinHandle<()>,
    ctxt: rusb::Context,
    ctxt_thread: Option<JoinHandle<()>>,
    quit: bool,
}

#[derive(Clone, Debug)]
struct Handler {
    inner: Arc<Mutex<InnerHandler>>,
}

impl DeviceHandler for Handler {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut inner = self.inner.lock().unwrap();
        let read = match fd_poll_readable(inner.stream.as_raw_fd(), false) {
            Ok(true) => {
                let read = inner.stream.read(buf);
                if let Ok(0) = read {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "disconnected",
                    ))
                } else {
                    read
                }
            }
            Ok(false) => Ok(0),
            Err(e) => Err(e),
        };

        inner.quit = read.is_err();
        read
    }

    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut inner = self.inner.lock().unwrap();
        let write = inner.stream.write_all(buf);
        inner.quit = write.is_err();
        write?;
        Ok(buf.len())
    }

    fn log(&mut self, _level: LogLevel, _msg: &str) {}

    fn flush_writes(&mut self) {}
}

#[zbus::dbus_proxy(
    interface = "org.freedesktop.usbredir1",
    default_service = "org.freedesktop.usbredir1",
    default_path = "/org/freedesktop/usbredir1"
)]
trait SystemHelper {
    fn open_bus_dev(&self, bus: u8, dev: u8) -> zbus::fdo::Result<zbus::zvariant::OwnedFd>;
}

impl Handler {
    async fn new(device: &rusb::Device<rusb::Context>, chardev: &Chardev) -> Result<Self> {
        let ctxt = device.context().clone();

        let (dev, device_fd) = match device.open() {
            Ok(it) => (it, None),
            Err(rusb::Error::Access) => {
                let (bus, dev) = (device.bus_number(), device.address());
                let sysbus = zbus::azync::Connection::system().await?;
                let fd = AsyncSystemHelperProxy::new(&sysbus)
                    .await?
                    .open_bus_dev(bus, dev)
                    .await?;
                unsafe { (ctxt.open_device_with_fd(fd.as_raw_fd())?, Some(fd)) }
            }
            Err(e) => {
                return Err(e.into());
            }
        };

        let (stream, peer) = UnixStream::pair()?;
        chardev.proxy.register(dbg!(peer.as_raw_fd()).into()).await?;

        let c = ctxt.clone();
        let stream_fd = stream.as_raw_fd();
        dbg!(stream_fd);
        // really annoying libusb/usbredir APIs...
        let stream_thread = std::thread::spawn(move || loop {
            let ret = fd_poll_readable(stream_fd, true);
            c.interrupt_handle_events();
            if ret.is_err() {
                dbg!();
                break;
            }
        });

        let handler = Self {
            inner: Arc::new(Mutex::new(InnerHandler {
                device_fd,
                stream,
                stream_thread,
                quit: false,
                ctxt: ctxt.clone(),
                ctxt_thread: Default::default(),
            })),
        };

        let redirdev = Device::new(&ctxt, Some(dev), handler.clone(), LogLevel::None as _)?;
        let c = ctxt.clone();
        let inner = handler.inner.clone();
        let ctxt_thread = std::thread::spawn(move || loop {
            if inner.lock().unwrap().quit {
                dbg!();
                break;
            }
            if let Ok(true) = fd_poll_readable(stream_fd, false) {
                redirdev.read_peer().unwrap();
            }
            if redirdev.has_data_to_write() > 0 {
                redirdev.write_peer().unwrap();
            }
            c.handle_events(None).unwrap();
        });
        handler
            .inner
            .lock()
            .unwrap()
            .ctxt_thread
            .replace(ctxt_thread);

        Ok(handler)
    }
}

impl Drop for InnerHandler {
    fn drop(&mut self) {
        //FIXME: for some reason close stream doesn't HUP qemu ??
        dbg!()
    }
}

impl Drop for Handler {
    fn drop(&mut self) {
        let mut inner = self.inner.lock().unwrap();
        inner.quit = true;
        inner.ctxt.interrupt_handle_events();
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct Key(u8, u8);

impl Key {
    fn from_device(device: &rusb::Device<rusb::Context>) -> Self {
        Self(device.bus_number(), device.address())
    }
}

#[derive(Debug)]
struct Inner {
    chardevs: Vec<Chardev>,
    handlers: HashMap<Key, Handler>,
}

impl Inner {
    async fn available_chardev(&self) -> Option<&Chardev> {
        for c in &self.chardevs {
            if c.proxy.owner().await.unwrap_or_default().is_empty() {
                return Some(c);
            }
        }
        None
    }
}

#[derive(Clone, Debug)]
pub struct UsbRedir {
    inner: Arc<RefCell<Inner>>,
}

impl UsbRedir {
    pub fn new(chardevs: Vec<Chardev>) -> Self {
        Self {
            inner: Arc::new(RefCell::new(Inner {
                chardevs,
                handlers: Default::default(),
            })),
        }
    }

    pub async fn set_device_state(
        &self,
        device: &rusb::Device<rusb::Context>,
        state: bool,
    ) -> Result<bool> {
        let mut inner = self.inner.borrow_mut();
        let key = Key::from_device(device);

        if state {
            if !inner.handlers.contains_key(&key) {
                let chardev = inner
                    .available_chardev()
                    .await
                    .ok_or_else(|| Error::Failed("There are no free USB channels".into()))?;
                let handler = Handler::new(device, chardev).await?;
                inner.handlers.insert(key, handler);
            }
        } else {
            inner.handlers.remove(&key);
        }

        Ok(state)
    }

    pub fn is_device_connected(&self, device: &rusb::Device<rusb::Context>) -> bool {
        let inner = self.inner.borrow();

        inner.handlers.contains_key(&Key::from_device(device))
    }
}

fn fd_poll_readable(fd: RawFd, wait: bool) -> std::io::Result<bool> {
    let mut fds = [libc::pollfd {
        fd,
        events: libc::POLLIN|libc::POLLHUP,
        revents: 0,
    }];
    let ret = unsafe { libc::poll(fds.as_mut_ptr(), 1, if wait { -1 } else { 0 }) };
    if ret > 0 {
        if fds[0].revents & libc::POLLHUP != 0 {
            Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "hup"))
        } else {
            Ok(fds[0].revents & libc::POLLIN != 0)
        }
    } else if ret == 0 {
        Ok(false)
    } else {
        Err(std::io::Error::last_os_error())
    }
}
