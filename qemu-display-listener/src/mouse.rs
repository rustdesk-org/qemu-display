use serde_repr::{Deserialize_repr, Serialize_repr};
use zbus::dbus_proxy;
use zvariant::derive::Type;

#[repr(u32)]
#[derive(Deserialize_repr, Serialize_repr, Type, Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    Side,
    Extra,
}

#[dbus_proxy(default_service = "org.qemu", interface = "org.qemu.Display1.Mouse")]
pub trait Mouse {
    /// Press method
    fn press(&self, button: MouseButton) -> zbus::Result<()>;

    /// Release method
    fn release(&self, button: MouseButton) -> zbus::Result<()>;

    /// SetAbsPosition method
    fn set_abs_position(&self, x: u32, y: u32) -> zbus::Result<()>;

    #[dbus_proxy(property)]
    fn is_absolute(&self) -> zbus::Result<bool>;
}
