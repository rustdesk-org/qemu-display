use serde_repr::{Deserialize_repr, Serialize_repr};
use std::convert::TryFrom;
use zbus::{dbus_interface, dbus_proxy, zvariant::ObjectPath};
use zvariant::Type;

use crate::Result;

#[repr(u32)]
#[derive(Deserialize_repr, Serialize_repr, Type, Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub enum ClipboardSelection {
    Clipboard,
    Primary,
    Secondary,
}

#[dbus_proxy(
    default_service = "org.qemu",
    default_path = "/org/qemu/Display1/Clipboard",
    interface = "org.qemu.Display1.Clipboard"
)]
pub trait Clipboard {
    fn register(&self) -> zbus::Result<()>;

    fn unregister(&self) -> zbus::Result<()>;

    fn grab(&self, selection: ClipboardSelection, serial: u32, mimes: &[&str]) -> zbus::Result<()>;

    fn release(&self, selection: ClipboardSelection) -> zbus::Result<()>;

    fn request(
        &self,
        selection: ClipboardSelection,
        mimes: &[&str],
    ) -> zbus::Result<(String, Vec<u8>)>;
}

#[async_trait::async_trait]
pub trait ClipboardHandler: 'static + Send + Sync {
    async fn register(&mut self);

    async fn unregister(&mut self);

    async fn grab(&mut self, selection: ClipboardSelection, serial: u32, mimes: Vec<String>);

    async fn release(&mut self, selection: ClipboardSelection);

    async fn request(
        &mut self,
        selection: ClipboardSelection,
        mimes: Vec<String>,
    ) -> Result<(String, Vec<u8>)>;
}

#[derive(Debug)]
pub(crate) struct ClipboardListener<H: ClipboardHandler> {
    handler: H,
}

#[dbus_interface(name = "org.qemu.Display1.Clipboard")]
impl<H: ClipboardHandler> ClipboardListener<H> {
    async fn register(&mut self) {
        self.handler.register().await;
    }

    async fn unregister(&mut self) {
        self.handler.unregister().await;
    }

    async fn grab(&mut self, selection: ClipboardSelection, serial: u32, mimes: Vec<String>) {
        self.handler.grab(selection, serial, mimes).await;
    }

    async fn release(&mut self, selection: ClipboardSelection) {
        self.handler.release(selection).await;
    }

    async fn request(
        &mut self,
        selection: ClipboardSelection,
        mimes: Vec<String>,
    ) -> zbus::fdo::Result<(String, Vec<u8>)> {
        self.handler
            .request(selection, mimes)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("Request failed: {}", e)))
    }
}

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct Clipboard {
    #[derivative(Debug = "ignore")]
    pub proxy: ClipboardProxy<'static>,
    conn: zbus::Connection,
}

impl Clipboard {
    pub async fn new(conn: &zbus::Connection) -> Result<Self> {
        let obj_path = ObjectPath::try_from("/org/qemu/Display1/Clipboard").unwrap();
        let proxy = ClipboardProxy::builder(conn)
            .path(&obj_path)?
            .build()
            .await?;
        Ok(Self {
            proxy,
            conn: conn.clone(),
        })
    }

    pub async fn register<H: ClipboardHandler>(&self, handler: H) -> Result<()> {
        self.conn
            .object_server()
            .at(
                "/org/qemu/Display1/Clipboard",
                ClipboardListener { handler },
            )
            .await
            .unwrap();
        Ok(self.proxy.register().await?)
    }
}
