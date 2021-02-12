use gtk::glib;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppError {
    GL = 1,
    Failed = 2,
}

impl glib::error::ErrorDomain for AppError {
    fn domain() -> glib::Quark {
        glib::Quark::from_string("qemu-gtk4")
    }

    fn code(self) -> i32 {
        self as _
    }

    fn from(code: i32) -> Option<Self>
    where
        Self: Sized,
    {
        use self::AppError::*;
        match code {
            x if x == GL as i32 => Some(GL),
            _ => Some(Failed),
        }
    }
}
