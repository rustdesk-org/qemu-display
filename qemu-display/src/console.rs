#[cfg(windows)]
use crate::win32::Fd;
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::{cell::RefCell, convert::TryFrom};
#[cfg(windows)]
use uds_windows::UnixStream;
#[cfg(unix)]
use zbus::zvariant::Fd;
use zbus::{dbus_proxy, zvariant::ObjectPath, Connection};

use crate::{util, ConsoleListener, ConsoleListenerHandler, KeyboardProxy, MouseProxy, Result};

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
    pub proxy: ConsoleProxy<'static>,
    #[derivative(Debug = "ignore")]
    pub keyboard: KeyboardProxy<'static>,
    #[derivative(Debug = "ignore")]
    pub mouse: MouseProxy<'static>,
    listener: RefCell<Option<Connection>>,
    #[cfg(windows)]
    peer_pid: u32,
}

impl Console {
    pub async fn new(conn: &Connection, idx: u32, #[cfg(windows)] peer_pid: u32) -> Result<Self> {
        let obj_path = ObjectPath::try_from(format!("/org/qemu/Display1/Console_{}", idx))?;
        let proxy = ConsoleProxy::builder(conn).path(&obj_path)?.build().await?;
        let keyboard = KeyboardProxy::builder(conn)
            .path(&obj_path)?
            .build()
            .await?;
        let mouse = MouseProxy::builder(conn).path(&obj_path)?.build().await?;
        Ok(Self {
            proxy,
            keyboard,
            mouse,
            listener: RefCell::new(None),
            #[cfg(windows)]
            peer_pid,
        })
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

    pub async fn register_listener<H: ConsoleListenerHandler>(&self, handler: H) -> Result<()> {
        let (p0, p1) = UnixStream::pair()?;
        let p0 = util::prepare_uds_pass(
            #[cfg(windows)]
            self.peer_pid,
            &p0,
        )?;
        self.proxy.register_listener(p0).await?;
        let c = zbus::ConnectionBuilder::unix_stream(p1)
            .p2p()
            .serve_at("/org/qemu/Display1/Listener", ConsoleListener::new(handler))?
            .build()
            .await?;
        self.listener.replace(Some(c));
        Ok(())
    }

    pub fn unregister_listener(&mut self) {
        self.listener.replace(None);
    }
}
