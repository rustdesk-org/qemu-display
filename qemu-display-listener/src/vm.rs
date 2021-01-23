use zbus::dbus_proxy;

#[dbus_proxy(
    default_service = "org.qemu",
    interface = "org.qemu.Display1.VM",
    default_path = "/org/qemu/Display1/VM"
)]
pub trait VM {
    /// Name property
    #[dbus_proxy(property)]
    fn name(&self) -> zbus::Result<String>;

    /// UUID property
    #[dbus_proxy(property)]
    fn uuid(&self) -> zbus::Result<String>;
}
