use std::error;
use std::fmt;
use std::io;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Zbus(zbus::Error),
    Zvariant(zvariant::Error),
    Failed(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{}", e),
            Error::Zbus(e) => write!(f, "{}", e),
            Error::Zvariant(e) => write!(f, "{}", e),
            Error::Failed(e) => write!(f, "{}", e),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Zbus(e) => Some(e),
            Error::Zvariant(e) => Some(e),
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

impl From<zvariant::Error> for Error {
    fn from(e: zvariant::Error) -> Self {
        Error::Zvariant(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
