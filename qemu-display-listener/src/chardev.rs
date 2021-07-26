use std::convert::TryFrom;
use zbus::dbus_proxy;
use zbus::export::zvariant::{Fd, ObjectPath};

use crate::Result;

#[dbus_proxy(default_service = "org.qemu", interface = "org.qemu.Display1.Chardev")]
pub trait Chardev {
    /// Register method
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
}

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct Chardev {
    #[derivative(Debug = "ignore")]
    pub proxy: AsyncChardevProxy<'static>,
}

impl Chardev {
    pub async fn new(conn: &zbus::azync::Connection, id: &str) -> Result<Self> {
        let obj_path = ObjectPath::try_from(format!("/org/qemu/Display1/Chardev_{}", id))?;
        let proxy = AsyncChardevProxy::builder(conn)
            .path(&obj_path)?
            .build_async()
            .await?;
        Ok(Self { proxy })
    }
}
