#[cfg(windows)]
use crate::win32::Fd;
use std::convert::TryFrom;
#[cfg(unix)]
use zbus::zvariant::Fd;
use zbus::{dbus_proxy, zvariant::ObjectPath};

use crate::Result;

#[dbus_proxy(default_service = "org.qemu", interface = "org.qemu.Display1.Chardev")]
pub trait Chardev {
    /// Register method
    #[cfg(unix)]
    fn register(&self, stream: Fd) -> zbus::Result<()>;

    /// SendBreak method
    fn send_break(&self) -> zbus::Result<()>;

    /// Echo property
    #[dbus_proxy(property)]
    fn echo(&self) -> zbus::Result<bool>;

    /// FEOpened property
    #[dbus_proxy(property, name = "FEOpened")]
    fn fe_opened(&self) -> zbus::Result<bool>;

    /// Name property
    #[dbus_proxy(property)]
    fn name(&self) -> zbus::Result<String>;

    /// Owner property
    #[dbus_proxy(property)]
    fn owner(&self) -> zbus::Result<String>;
}

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct Chardev {
    pub proxy: ChardevProxy<'static>,
}

impl Chardev {
    pub async fn new(conn: &zbus::Connection, id: &str) -> Result<Self> {
        let obj_path = ObjectPath::try_from(format!("/org/qemu/Display1/Chardev_{}", id))?;
        let proxy = ChardevProxy::builder(conn).path(&obj_path)?.build().await?;
        Ok(Self { proxy })
    }
}
