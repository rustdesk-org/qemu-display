use std::{
    cell::RefCell,
    convert::TryFrom,
    os::unix::{io::AsRawFd, net::UnixStream},
};
use zbus::{
    dbus_proxy,
    zvariant::{Fd, ObjectPath},
    Connection,
};

use crate::{AsyncKeyboardProxy, AsyncMouseProxy, ConsoleListener, ConsoleListenerHandler, Result};

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
    listener: RefCell<Option<Connection>>,
}

impl Console {
    pub async fn new(conn: &Connection, idx: u32) -> Result<Self> {
        let obj_path = ObjectPath::try_from(format!("/org/qemu/Display1/Console_{}", idx))?;
        let proxy = AsyncConsoleProxy::builder(conn)
            .path(&obj_path)?
            .build()
            .await?;
        let keyboard = AsyncKeyboardProxy::builder(conn)
            .path(&obj_path)?
            .build()
            .await?;
        let mouse = AsyncMouseProxy::builder(conn)
            .path(&obj_path)?
            .build()
            .await?;
        Ok(Self {
            proxy,
            keyboard,
            mouse,
            listener: RefCell::new(None),
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
        self.proxy.register_listener(p0.as_raw_fd().into()).await?;
        let c = zbus::ConnectionBuilder::unix_stream(p1)
            .p2p()
            .build()
            .await?;
        {
            let mut server = c.object_server_mut().await;
            server
                .at("/org/qemu/Display1/Listener", ConsoleListener::new(handler))
                .unwrap();
            server.start_dispatch();
        }
        self.listener.replace(Some(c));
        Ok(())
    }

    pub fn unregister_listener(&mut self) {
        self.listener.replace(None);
    }
}
