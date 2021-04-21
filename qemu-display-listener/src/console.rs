use std::os::unix::net::UnixStream;
use std::sync::mpsc::{self, Receiver, Sender};
use std::{os::unix::io::AsRawFd, thread};

use zbus::{dbus_proxy, export::zvariant::Fd};

use crate::Result;
use crate::{ConsoleEvent, ConsoleListener, AsyncKeyboardProxy, AsyncMouseProxy};

#[dbus_proxy(default_service = "org.qemu", interface = "org.qemu.Display1.Console")]
pub trait Console {
    /// RegisterListener method
    fn register_listener(&self, listener: Fd) -> zbus::Result<()>;

    /// SetUIInfo method
    #[dbus_proxy(name = "SetUIInfo")]
    fn set_ui_info(
        &self,
        width_mm: u16,
        height_mm: u16,
        xoff: i32,
        yoff: i32,
        width: u32,
        height: u32,
    ) -> zbus::Result<()>;

    #[dbus_proxy(property)]
    fn label(&self) -> zbus::Result<String>;

    #[dbus_proxy(property)]
    fn head(&self) -> zbus::Result<u32>;

    #[dbus_proxy(property)]
    fn type_(&self) -> zbus::Result<String>;

    #[dbus_proxy(property)]
    fn width(&self) -> zbus::Result<u32>;

    #[dbus_proxy(property)]
    fn height(&self) -> zbus::Result<u32>;
}

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct Console {
    #[derivative(Debug = "ignore")]
    pub proxy: AsyncConsoleProxy<'static>,
    #[derivative(Debug = "ignore")]
    pub keyboard: AsyncKeyboardProxy<'static>,
    #[derivative(Debug = "ignore")]
    pub mouse: AsyncMouseProxy<'static>,
}

impl Console {
    pub async fn new(conn: &zbus::azync::Connection, idx: u32) -> Result<Self> {
        let obj_path = format!("/org/qemu/Display1/Console_{}", idx);
        let mut proxy = AsyncConsoleProxy::new_for_owned_path(conn.clone(), obj_path.clone())?;
        proxy.cache_properties().await;
        let mut keyboard = AsyncKeyboardProxy::new_for_owned_path(conn.clone(), obj_path.clone())?;
        keyboard.cache_properties().await;
        let mut mouse = AsyncMouseProxy::new_for_owned_path(conn.clone(), obj_path)?;
        mouse.cache_properties().await;
        Ok(Self {
            proxy,
            keyboard,
            mouse,
        })
    }

    pub async fn dispatch_signals(&self) -> Result<()> {
        use futures_util::{future::FutureExt, select};

        select!(
            msg = self.proxy.next_signal().fuse() => {
                if let Some(msg) = msg? {
                    log::debug!("Ignoring {:?}", msg);
                }
                return Ok(());
            },
            msg = self.keyboard.next_signal().fuse() => {
                if let Some(msg) = msg? {
                    log::debug!("Ignoring {:?}", msg);
                }
                return Ok(());
            },
            msg = self.mouse.next_signal().fuse() => {
                if let Some(msg) = msg? {
                    log::debug!("Ignoring {:?}", msg);
                }
                return Ok(());
            }
        );
    }

    pub async fn label(&self) -> Result<String> {
        Ok(self.proxy.label().await?)
    }

    pub async fn width(&self) -> Result<u32> {
        Ok(self.proxy.width().await?)
    }

    pub async fn height(&self) -> Result<u32> {
        Ok(self.proxy.height().await?)
    }

    pub async fn listen(&self) -> Result<(Receiver<ConsoleEvent>, Sender<()>)> {
        let (p0, p1) = UnixStream::pair()?;
        let (tx, rx) = mpsc::channel();
        self.proxy.register_listener(p0.as_raw_fd().into()).await?;

        let (wait_tx, wait_rx) = mpsc::channel();
        let _thread = thread::spawn(move || {
            let c = zbus::Connection::new_unix_client(p1, false).unwrap();
            let mut s = zbus::ObjectServer::new(&c);
            let listener = ConsoleListener::new(tx, wait_rx);
            let err = listener.err();
            s.at("/org/qemu/Display1/Listener", listener).unwrap();
            loop {
                if let Err(e) = s.try_handle_next() {
                    eprintln!("Listener DBus error: {}", e);
                    return;
                }
                if let Some(e) = &*err.borrow() {
                    eprintln!("Listener channel error: {}", e);
                    return;
                }
            }
        });

        Ok((rx, wait_tx))
    }
}

#[cfg(feature = "glib")]
impl Console {
    pub async fn glib_listen(&self) -> Result<(glib::Receiver<ConsoleEvent>, Sender<()>)> {
        let (p0, p1) = UnixStream::pair()?;
        let (tx, rx) = glib::MainContext::channel(glib::source::Priority::default());
        self.proxy.register_listener(p0.as_raw_fd().into()).await?;

        let (wait_tx, wait_rx) = mpsc::channel();
        let _thread = thread::spawn(move || {
            let c = zbus::Connection::new_unix_client(p1, false).unwrap();
            let mut s = zbus::ObjectServer::new(&c);
            let listener = ConsoleListener::new(tx, wait_rx);
            let err = listener.err();
            s.at("/org/qemu/Display1/Listener", listener).unwrap();
            loop {
                if let Err(e) = s.try_handle_next() {
                    eprintln!("Listener DBus error: {}", e);
                    break;
                }
                if let Some(e) = &*err.borrow() {
                    eprintln!("Listener channel error: {}", e);
                    break;
                }
            }
        });

        Ok((rx, wait_tx))
    }
}
