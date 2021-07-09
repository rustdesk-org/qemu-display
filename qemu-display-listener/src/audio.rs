use once_cell::sync::OnceCell;
use std::default::Default;
use std::os::unix::net::UnixStream;
use std::str::FromStr;
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

impl PCMInfo {
    pub fn gst_caps(&self) -> String {
        let format = format!(
            "{}{}{}",
            if self.is_float {
                "F"
            } else if self.is_signed {
                "S"
            } else {
                "U"
            },
            self.bits,
            if self.be { "BE" } else { "LE" }
        );
        format!(
            "audio/x-raw,format={format},channels={channels},rate={rate},layout=interleaved",
            format = format,
            channels = self.nchannels,
            rate = self.freq,
        )
    }
}

#[derive(Debug)]
pub struct Volume {
    pub mute: bool,
    pub volume: Vec<u8>,
}

#[derive(Debug)]
pub enum AudioOutEvent {
    Init { id: u64, info: PCMInfo },
    Fini { id: u64 },
    SetEnabled { id: u64, enabled: bool },
    SetVolume { id: u64, volume: Volume },
    Write { id: u64, data: Vec<u8> },
}

#[derive(Debug)]
pub enum AudioInEvent {
    Init { id: u64, info: PCMInfo },
    Fini { id: u64 },
    SetEnabled { id: u64, enabled: bool },
    SetVolume { id: u64, volume: Volume },
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
    pub proxy: AsyncAudioProxy<'static>,
}

#[derive(Debug)]
pub(crate) struct AudioOutListener<E: EventSender<Event = AudioOutEvent>> {
    tx: E,
    err: Arc<OnceCell<SendError<AudioOutEvent>>>,
}

impl<E: EventSender<Event = AudioOutEvent>> AudioOutListener<E> {
    pub(crate) fn new(tx: E) -> Self {
        AudioOutListener {
            tx,
            err: Default::default(),
        }
    }

    fn send(&mut self, event: AudioOutEvent) {
        if let Err(e) = self.tx.send_event(event) {
            let _ = self.err.set(e);
        }
    }

    pub fn err(&self) -> Arc<OnceCell<SendError<AudioOutEvent>>> {
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

    /// SetVolume method
    fn set_volume(&mut self, id: u64, mute: bool, volume: serde_bytes::ByteBuf) {
        self.send(AudioOutEvent::SetVolume {
            id,
            volume: Volume {
                mute,
                volume: volume.into_vec(),
            },
        });
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
    err: Arc<OnceCell<SendError<AudioInEvent>>>,
}

impl<E: EventSender<Event = AudioInEvent>> AudioInListener<E> {
    pub(crate) fn new(tx: E) -> Self {
        AudioInListener {
            tx,
            err: Default::default(),
        }
    }

    fn send(&mut self, event: AudioInEvent) {
        if let Err(e) = self.tx.send_event(event) {
            let _ = self.err.set(e);
        }
    }

    pub fn err(&self) -> Arc<OnceCell<SendError<AudioInEvent>>> {
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

    /// SetVolume method
    fn set_volume(&mut self, id: u64, mute: bool, volume: serde_bytes::ByteBuf) {
        self.send(AudioInEvent::SetVolume {
            id,
            volume: Volume {
                mute,
                volume: volume.into_vec(),
            },
        });
    }

    /// Read method
    fn read(&mut self, id: u64, size: u64) -> Vec<u8> {
        dbg!((id, size));
        vec![0; size as usize]
    }
}

impl Audio {
    pub async fn new(conn: &zbus::azync::Connection) -> Result<Self> {
        let proxy = AsyncAudioProxy::new(conn).await?;
        Ok(Self { proxy })
    }

    pub async fn available(conn: &zbus::azync::Connection) -> bool {
        // TODO: we may want to generalize interface detection
        let ip = zbus::fdo::AsyncIntrospectableProxy::builder(conn)
            .destination("org.qemu")
            .path("/org/qemu/Display1")
            .unwrap()
            .build_async()
            .await
            .unwrap();
        let introspect = zbus::xml::Node::from_str(&ip.introspect().await.unwrap()).unwrap();
        let has_audio = introspect
            .nodes()
            .iter()
            .any(|n| n.name().map(|n| n == "Audio").unwrap_or(false));
        has_audio
    }

    pub async fn listen_out(&self) -> Result<Receiver<AudioOutEvent>> {
        let (p0, p1) = UnixStream::pair()?;
        let (tx, rx) = mpsc::channel();
        self.proxy
            .register_out_listener(p0.as_raw_fd().into())
            .await?;

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
                if let Some(e) = err.get() {
                    eprintln!("Listener channel error: {}", e);
                    return;
                }
            }
        });

        Ok(rx)
    }

    pub async fn listen_in(&self) -> Result<Receiver<AudioInEvent>> {
        let (p0, p1) = UnixStream::pair()?;
        let (tx, rx) = mpsc::channel();
        self.proxy
            .register_in_listener(p0.as_raw_fd().into())
            .await?;

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
                if let Some(e) = err.get() {
                    eprintln!("Listener channel error: {}", e);
                    return;
                }
            }
        });

        Ok(rx)
    }
}
