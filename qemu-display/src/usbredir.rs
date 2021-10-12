use async_broadcast::{broadcast, Receiver, Sender};
use async_lock::RwLock;
use futures::Stream;
use std::{
    collections::HashMap,
    default::Default,
    io::{Read, Write},
    os::unix::{
        io::{AsRawFd, RawFd},
        net::UnixStream,
    },
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    thread::JoinHandle,
};
use usbredirhost::{
    rusb::{self, UsbContext},
    Device, DeviceHandler, LogLevel,
};

use crate::{Chardev, Error, Result};

#[derive(Debug)]
struct InnerHandler {
    #[allow(unused)] // keep the device opened, as rusb doesn't take it
    device_fd: Option<zvariant::OwnedFd>,
    stream: UnixStream,
    ctxt: rusb::Context,
    ctxt_thread: Option<JoinHandle<()>>,
    event: (UnixStream, UnixStream),
    quit: bool,
}

#[derive(Clone, Debug)]
struct Handler {
    inner: Arc<Mutex<InnerHandler>>,
}

impl DeviceHandler for Handler {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut inner = self.inner.lock().unwrap();
        let read = match fd_poll_readable(inner.stream.as_raw_fd(), None) {
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
                let sysbus = zbus::Connection::system().await?;
                let fd = SystemHelperProxy::new(&sysbus)
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
        chardev.proxy.register(peer.as_raw_fd().into()).await?;

        let c = ctxt.clone();
        let stream_fd = stream.as_raw_fd();
        // really annoying libusb/usbredir APIs...
        let event = UnixStream::pair()?;
        let event_fd = event.1.as_raw_fd();
        std::thread::spawn(move || loop {
            let ret = fd_poll_readable(stream_fd, Some(event_fd));
            c.interrupt_handle_events();
            if ret.is_err() {
                break;
            }
        });

        let handler = Self {
            inner: Arc::new(Mutex::new(InnerHandler {
                device_fd,
                stream,
                event,
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
                break;
            }
            if let Ok(true) = fd_poll_readable(stream_fd, None) {
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

impl Drop for Handler {
    fn drop(&mut self) {
        let mut inner = self.inner.lock().unwrap();
        inner.quit = true;
        inner.ctxt.interrupt_handle_events();
        // stream will be dropped and stream_thread will kick context_thread
        inner.event.0.write_all(&[0]).unwrap();
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct Key(u8, u8);

impl Key {
    fn from_device(device: &rusb::Device<rusb::Context>) -> Self {
        Self(device.bus_number(), device.address())
    }
}

#[derive(Debug, Clone, Copy)]
enum Event {
    NFreeChannels(i32),
}

#[derive(Debug)]
struct Inner {
    chardevs: Vec<Chardev>,
    handlers: HashMap<Key, Handler>,
    channel: (Sender<Event>, Receiver<Event>),
}

impl Inner {
    // could make use of async combinators..
    async fn first_available_chardev(&self) -> Option<&Chardev> {
        for c in &self.chardevs {
            if c.proxy.owner().await.unwrap_or_default().is_empty() {
                return Some(c);
            }
        }
        None
    }

    async fn n_available_chardev(&self) -> usize {
        let mut n = 0;
        for c in &self.chardevs {
            if c.proxy.owner().await.unwrap_or_default().is_empty() {
                n += 1;
            }
        }
        n
    }
}

#[derive(Clone, Debug)]
pub struct UsbRedir {
    inner: Arc<RwLock<Inner>>,
}

impl UsbRedir {
    pub fn new(chardevs: Vec<Chardev>) -> Self {
        let mut channel = broadcast(1);
        channel.0.set_overflow(true);
        Self {
            inner: Arc::new(RwLock::new(Inner {
                chardevs,
                channel,
                handlers: Default::default(),
            })),
        }
    }

    pub async fn set_device_state(
        &self,
        device: &rusb::Device<rusb::Context>,
        state: bool,
    ) -> Result<bool> {
        let mut inner = self.inner.write().await;
        let key = Key::from_device(device);
        let handled = inner.handlers.contains_key(&key);
        // We should do better and watch for owner properties changes, but this would require tasks
        // anticipate result
        let mut nfree = inner.n_available_chardev().await as _;

        match (state, handled) {
            (true, false) => {
                let chardev = inner
                    .first_available_chardev()
                    .await
                    .ok_or_else(|| Error::Failed("There are no free USB channels".into()))?;
                let handler = Handler::new(device, chardev).await?;
                inner.handlers.insert(key, handler);
                nfree -= 1;
            }
            (false, true) => {
                inner.handlers.remove(&key);
                nfree += 1;
            }
            _ => {
                return Ok(state);
            }
        }

        let _ = inner.channel.0.broadcast(Event::NFreeChannels(nfree)).await;

        Ok(state)
    }

    pub async fn is_device_connected(&self, device: &rusb::Device<rusb::Context>) -> bool {
        let inner = self.inner.read().await;

        inner.handlers.contains_key(&Key::from_device(device))
    }

    pub async fn n_free_channels(&self) -> i32 {
        let inner = self.inner.read().await;

        inner.n_available_chardev().await as _
    }

    pub async fn receive_n_free_channels(&self) -> Pin<Box<dyn Stream<Item = i32>>> {
        let inner = self.inner.read().await;

        Box::pin(NFreeChannelsStream {
            receiver: inner.channel.1.clone(),
        })
    }
}

#[derive(Debug)]
struct NFreeChannelsStream {
    receiver: Receiver<Event>,
}

impl Stream for NFreeChannelsStream {
    type Item = i32;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = Pin::new(self);

        match Stream::poll_next(Pin::new(&mut this.receiver), cx) {
            Poll::Ready(Some(Event::NFreeChannels(n))) => Poll::Ready(Some(n)),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

fn fd_poll_readable(fd: RawFd, wait: Option<RawFd>) -> std::io::Result<bool> {
    let mut fds = vec![libc::pollfd {
        fd,
        events: libc::POLLIN | libc::POLLHUP,
        revents: 0,
    }];
    if let Some(wait) = wait {
        fds.push(libc::pollfd {
            fd: wait,
            events: libc::POLLIN | libc::POLLHUP,
            revents: 0,
        });
    }
    let ret = unsafe {
        libc::poll(
            fds.as_mut_ptr(),
            fds.len() as _,
            if wait.is_some() { -1 } else { 0 },
        )
    };
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else if ret == 0 {
        Ok(false)
    } else if fds[0].revents & libc::POLLHUP != 0
        || (wait.is_some() && fds[1].revents & libc::POLLIN != 0)
    {
        Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "hup"))
    } else {
        Ok(fds[0].revents & libc::POLLIN != 0)
    }
}
