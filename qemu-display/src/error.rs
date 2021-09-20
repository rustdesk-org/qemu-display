use usbredirhost::rusb;

use std::{convert::Infallible, error, fmt, io};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Zbus(zbus::Error),
    Rusb(rusb::Error),
    Usbredir(usbredirhost::Error),
    Failed(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "IO error: {}", e),
            Error::Zbus(e) => write!(f, "zbus error: {}", e),
            Error::Rusb(e) => write!(f, "rusb error: {}", e),
            Error::Usbredir(e) => write!(f, "usbredir error: {}", e),
            Error::Failed(e) => write!(f, "{}", e),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Zbus(e) => Some(e),
            Error::Rusb(e) => Some(e),
            Error::Usbredir(e) => Some(e),
            Error::Failed(_) => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<zbus::Error> for Error {
    fn from(e: zbus::Error) -> Self {
        Error::Zbus(e)
    }
}

impl From<zbus::fdo::Error> for Error {
    fn from(e: zbus::fdo::Error) -> Self {
        Error::Zbus(e.into())
    }
}

impl From<zvariant::Error> for Error {
    fn from(e: zvariant::Error) -> Self {
        Error::Zbus(e.into())
    }
}

impl From<zbus::names::Error> for Error {
    fn from(e: zbus::names::Error) -> Self {
        Error::Zbus(e.into())
    }
}

impl From<rusb::Error> for Error {
    fn from(e: rusb::Error) -> Self {
        Error::Rusb(e)
    }
}

impl From<usbredirhost::Error> for Error {
    fn from(e: usbredirhost::Error) -> Self {
        Error::Usbredir(e)
    }
}

impl From<Infallible> for Error {
    fn from(_: Infallible) -> Self {
        unreachable!()
    }
}

pub type Result<T> = std::result::Result<T, Error>;
