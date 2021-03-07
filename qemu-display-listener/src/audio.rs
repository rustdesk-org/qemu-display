use std::cell::RefCell;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{self, Receiver, SendError};
use std::sync::Arc;
use std::{os::unix::io::AsRawFd, thread};

use zbus::{dbus_interface, dbus_proxy, export::zvariant::Fd};

use crate::{EventSender, Result};

#[derive(Debug)]
pub struct PCMInfo {
    pub bits: u8,
    pub is_signed: bool,
    pub is_float: bool,
    pub freq: u32,
    pub nchannels: u8,
    pub bytes_per_frame: u32,
    pub bytes_per_second: u32,
    pub be: bool,
}

#[derive(Debug)]
pub enum AudioOutEvent {
    Init { id: u64, info: PCMInfo },
    Fini { id: u64 },
    SetEnabled { id: u64, enabled: bool },
    Write { id: u64, data: Vec<u8> },
}

#[derive(Debug)]
pub enum AudioInEvent {
    Init { id: u64, info: PCMInfo },
    Fini { id: u64 },
    SetEnabled { id: u64, enabled: bool },
    Read { id: u64 },
}

#[dbus_proxy(
    default_service = "org.qemu",
    default_path = "/org/qemu/Display1/Audio",
    interface = "org.qemu.Display1.Audio"
)]
trait Audio {
    /// RegisterOutListener method
    fn register_out_listener(&self, listener: Fd) -> zbus::Result<()>;

    /// RegisterInListener method
    fn register_in_listener(&self, listener: Fd) -> zbus::Result<()>;
}

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct Audio {
    #[derivative(Debug = "ignore")]
    pub proxy: AudioProxy<'static>,
}

#[derive(Debug)]
pub(crate) struct AudioOutListener<E: EventSender<Event = AudioOutEvent>> {
    tx: E,
    err: Arc<RefCell<Option<SendError<AudioOutEvent>>>>,
}

impl<E: EventSender<Event = AudioOutEvent>> AudioOutListener<E> {
    pub(crate) fn new(tx: E) -> Self {
        let err = Arc::new(RefCell::new(None));
        AudioOutListener { tx, err }
    }

    fn send(&mut self, event: AudioOutEvent) {
        if let Err(e) = self.tx.send_event(event) {
            *self.err.borrow_mut() = Some(e);
        }
    }

    pub fn err(&self) -> Arc<RefCell<Option<SendError<AudioOutEvent>>>> {
        self.err.clone()
    }
}

#[dbus_interface(name = "org.qemu.Display1.AudioOutListener")]
impl<E: 'static + EventSender<Event = AudioOutEvent>> AudioOutListener<E> {
    /// Init method
    fn init(
        &mut self,
        id: u64,
        bits: u8,
        is_signed: bool,
        is_float: bool,
        freq: u32,
        nchannels: u8,
        bytes_per_frame: u32,
        bytes_per_second: u32,
        be: bool,
    ) {
        self.send(AudioOutEvent::Init {
            id,
            info: PCMInfo {
                bits,
                is_signed,
                is_float,
                freq,
                nchannels,
                bytes_per_frame,
                bytes_per_second,
                be,
            },
        })
    }

    /// Fini method
    fn fini(&mut self, id: u64) {
        self.send(AudioOutEvent::Fini { id })
    }

    /// SetEnabled method
    fn set_enabled(&mut self, id: u64, enabled: bool) {
        self.send(AudioOutEvent::SetEnabled { id, enabled })
    }

    /// Write method
    fn write(&mut self, id: u64, data: serde_bytes::ByteBuf) {
        self.send(AudioOutEvent::Write {
            id,
            data: data.into_vec(),
        })
    }
}

#[derive(Debug)]
pub(crate) struct AudioInListener<E: EventSender<Event = AudioInEvent>> {
    tx: E,
    err: Arc<RefCell<Option<SendError<AudioInEvent>>>>,
}

impl<E: EventSender<Event = AudioInEvent>> AudioInListener<E> {
    pub(crate) fn new(tx: E) -> Self {
        let err = Arc::new(RefCell::new(None));
        AudioInListener { tx, err }
    }

    fn send(&mut self, event: AudioInEvent) {
        if let Err(e) = self.tx.send_event(event) {
            *self.err.borrow_mut() = Some(e);
        }
    }

    pub fn err(&self) -> Arc<RefCell<Option<SendError<AudioInEvent>>>> {
        self.err.clone()
    }
}

#[dbus_interface(name = "org.qemu.Display1.AudioInListener")]
impl<E: 'static + EventSender<Event = AudioInEvent>> AudioInListener<E> {
    /// Init method
    fn init(
        &mut self,
        id: u64,
        bits: u8,
        is_signed: bool,
        is_float: bool,
        freq: u32,
        nchannels: u8,
        bytes_per_frame: u32,
        bytes_per_second: u32,
        be: bool,
    ) {
        self.send(AudioInEvent::Init {
            id,
            info: PCMInfo {
                bits,
                is_signed,
                is_float,
                freq,
                nchannels,
                bytes_per_frame,
                bytes_per_second,
                be,
            },
        })
    }

    /// Fini method
    fn fini(&mut self, id: u64) {
        self.send(AudioInEvent::Fini { id })
    }

    /// SetEnabled method
    fn set_enabled(&mut self, id: u64, enabled: bool) {
        self.send(AudioInEvent::SetEnabled { id, enabled })
    }

    /// Read method
    fn read(&mut self, id: u64, size: u64) -> Vec<u8> {
        dbg!((id, size));
        vec![0; size as usize]
    }
}

impl Audio {
    pub fn new(conn: &zbus::Connection) -> Result<Self> {
        let proxy = AudioProxy::new(conn)?;
        Ok(Self { proxy })
    }

    pub fn listen_out(&self) -> Result<Receiver<AudioOutEvent>> {
        let (p0, p1) = UnixStream::pair()?;
        let (tx, rx) = mpsc::channel();
        self.proxy.register_out_listener(p0.as_raw_fd().into())?;

        let _thread = thread::spawn(move || {
            let c = zbus::Connection::new_unix_client(p1, false).unwrap();
            let mut s = zbus::ObjectServer::new(&c);
            let listener = AudioOutListener::new(tx);
            let err = listener.err();
            s.at("/org/qemu/Display1/AudioOutListener", listener)
                .unwrap();
            loop {
                if let Err(e) = s.try_handle_next() {
                    eprintln!("Listener DBus error: {}", e);
                    return;
                }
                if let Some(e) = &*err.borrow() {
                    eprintln!("Listener channel error: {}", e);
                    return;
                }
            }
        });

        Ok(rx)
    }

    pub fn listen_in(&self) -> Result<Receiver<AudioInEvent>> {
        let (p0, p1) = UnixStream::pair()?;
        let (tx, rx) = mpsc::channel();
        self.proxy.register_in_listener(p0.as_raw_fd().into())?;

        let _thread = thread::spawn(move || {
            let c = zbus::Connection::new_unix_client(p1, false).unwrap();
            let mut s = zbus::ObjectServer::new(&c);
            let listener = AudioInListener::new(tx);
            let err = listener.err();
            s.at("/org/qemu/Display1/AudioInListener", listener)
                .unwrap();
            loop {
                if let Err(e) = s.try_handle_next() {
                    eprintln!("Listener DBus error: {}", e);
                    return;
                }
                if let Some(e) = &*err.borrow() {
                    eprintln!("Listener channel error: {}", e);
                    return;
                }
            }
        });

        Ok(rx)
    }
}
