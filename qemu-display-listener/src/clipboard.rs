use once_cell::sync::OnceCell;
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::convert::TryFrom;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use zbus::{dbus_interface, dbus_proxy, zvariant::ObjectPath};
use zvariant::derive::Type;

use crate::{EventSender, Result};

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

pub type ClipboardReplyTx = Sender<Result<(String, Vec<u8>)>>;

// TODO: replace events mpsc with async traits
#[derive(Debug)]
pub enum ClipboardEvent {
    Register,
    Unregister,
    Grab {
        selection: ClipboardSelection,
        serial: u32,
        mimes: Vec<String>,
    },
    Release {
        selection: ClipboardSelection,
    },
    Request {
        selection: ClipboardSelection,
        mimes: Vec<String>,
        tx: Mutex<ClipboardReplyTx>,
    },
}

#[derive(Debug)]
pub(crate) struct ClipboardListener<E: EventSender<Event = ClipboardEvent>> {
    tx: E,
    err: Arc<OnceCell<String>>,
}

#[dbus_interface(name = "org.qemu.Display1.Clipboard")]
impl<E: 'static + EventSender<Event = ClipboardEvent>> ClipboardListener<E> {
    fn register(&mut self) {
        self.send(ClipboardEvent::Register)
    }

    fn unregister(&mut self) {
        self.send(ClipboardEvent::Unregister)
    }

    fn grab(&mut self, selection: ClipboardSelection, serial: u32, mimes: Vec<String>) {
        self.send(ClipboardEvent::Grab {
            selection,
            serial,
            mimes,
        })
    }

    fn release(&mut self, selection: ClipboardSelection) {
        self.send(ClipboardEvent::Release { selection })
    }

    fn request(
        &mut self,
        selection: ClipboardSelection,
        mimes: Vec<String>,
    ) -> zbus::fdo::Result<(String, Vec<u8>)> {
        let (tx, rx) = channel();
        self.send(ClipboardEvent::Request {
            selection,
            mimes,
            tx: Mutex::new(tx),
        });
        rx.recv()
            .map_err(|e| zbus::fdo::Error::Failed(format!("Request recv failed: {}", e)))?
            .map_err(|e| zbus::fdo::Error::Failed(format!("Request failed: {}", e)))
    }
}

impl<E: 'static + EventSender<Event = ClipboardEvent>> ClipboardListener<E> {
    pub fn new(tx: E) -> Self {
        Self {
            tx,
            err: Default::default(),
        }
    }

    fn send(&mut self, event: ClipboardEvent) {
        if let Err(e) = self.tx.send_event(event) {
            let _ = self.err.set(e.to_string());
        }
    }

    pub fn err(&self) -> Arc<OnceCell<String>> {
        self.err.clone()
    }
}

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct Clipboard {
    conn: zbus::azync::Connection,
    #[derivative(Debug = "ignore")]
    pub proxy: AsyncClipboardProxy<'static>,
}

impl Clipboard {
    pub async fn new(conn: &zbus::azync::Connection) -> Result<Self> {
        let obj_path = ObjectPath::try_from("/org/qemu/Display1/Clipboard")?;
        let proxy = AsyncClipboardProxy::builder(conn)
            .path(&obj_path)?
            .build()
            .await?;
        Ok(Self {
            conn: conn.clone(),
            proxy,
        })
    }

    pub async fn register(&self) -> Result<()> {
        self.proxy.register().await?;
        Ok(())
    }
}

#[cfg(feature = "glib")]
impl Clipboard {
    pub async fn glib_listen(&self) -> Result<glib::Receiver<ClipboardEvent>> {
        let (tx, rx) = glib::MainContext::channel(glib::source::Priority::default());
        let c = self.conn.clone().into();
        let _thread = std::thread::spawn(move || {
            let mut s = zbus::ObjectServer::new(&c);
            let listener = ClipboardListener::new(tx);
            let err = listener.err();
            s.at("/org/qemu/Display1/Clipboard", listener).unwrap();
            loop {
                if let Err(e) = s.try_handle_next() {
                    eprintln!("Listener DBus error: {}", e);
                    break;
                }
                if let Some(e) = err.get() {
                    eprintln!("Listener channel error: {}", e);
                    break;
                }
            }
        });

        Ok(rx)
    }
}
