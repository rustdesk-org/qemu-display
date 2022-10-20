#[cfg(windows)]
use crate::win32::Fd;
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(windows)]
use uds_windows::UnixStream;
#[cfg(unix)]
use zbus::zvariant::Fd;
use zbus::{dbus_interface, dbus_proxy, Connection};

use crate::util;
use crate::Result;

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
    out_listener: Option<Connection>,
    in_listener: Option<Connection>,
    #[cfg(windows)]
    peer_pid: u32,
}

#[async_trait::async_trait]
pub trait AudioOutHandler: 'static + Send + Sync {
    async fn init(&mut self, id: u64, info: PCMInfo);

    async fn fini(&mut self, id: u64);

    async fn set_enabled(&mut self, id: u64, enabled: bool);

    async fn set_volume(&mut self, id: u64, volume: Volume);

    async fn write(&mut self, id: u64, data: Vec<u8>);
}

struct AudioOutListener<H: AudioOutHandler> {
    handler: H,
}

#[dbus_interface(name = "org.qemu.Display1.AudioOutListener")]
impl<H: AudioOutHandler> AudioOutListener<H> {
    /// Init method
    async fn init(
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
        self.handler
            .init(
                id,
                PCMInfo {
                    bits,
                    is_signed,
                    is_float,
                    freq,
                    nchannels,
                    bytes_per_frame,
                    bytes_per_second,
                    be,
                },
            )
            .await
    }

    /// Fini method
    async fn fini(&mut self, id: u64) {
        self.handler.fini(id).await
    }

    /// SetEnabled method
    async fn set_enabled(&mut self, id: u64, enabled: bool) {
        self.handler.set_enabled(id, enabled).await
    }

    /// SetVolume method
    async fn set_volume(&mut self, id: u64, mute: bool, volume: serde_bytes::ByteBuf) {
        self.handler
            .set_volume(
                id,
                Volume {
                    mute,
                    volume: volume.into_vec(),
                },
            )
            .await
    }

    /// Write method
    async fn write(&mut self, id: u64, data: serde_bytes::ByteBuf) {
        self.handler.write(id, data.into_vec()).await
    }
}

#[async_trait::async_trait]
pub trait AudioInHandler: 'static + Send + Sync {
    async fn init(&mut self, id: u64, info: PCMInfo);

    async fn fini(&mut self, id: u64);

    async fn set_enabled(&mut self, id: u64, enabled: bool);

    async fn set_volume(&mut self, id: u64, volume: Volume);

    async fn read(&mut self, id: u64, size: u64) -> Vec<u8>;
}

struct AudioInListener<H: AudioInHandler> {
    handler: H,
}

#[dbus_interface(name = "org.qemu.Display1.AudioInListener")]
impl<H: AudioInHandler> AudioInListener<H> {
    /// Init method
    async fn init(
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
        self.handler
            .init(
                id,
                PCMInfo {
                    bits,
                    is_signed,
                    is_float,
                    freq,
                    nchannels,
                    bytes_per_frame,
                    bytes_per_second,
                    be,
                },
            )
            .await
    }

    /// Fini method
    async fn fini(&mut self, id: u64) {
        self.handler.fini(id).await
    }

    /// SetEnabled method
    async fn set_enabled(&mut self, id: u64, enabled: bool) {
        self.handler.set_enabled(id, enabled).await
    }

    /// SetVolume method
    async fn set_volume(&mut self, id: u64, mute: bool, volume: serde_bytes::ByteBuf) {
        self.handler
            .set_volume(
                id,
                Volume {
                    mute,
                    volume: volume.into_vec(),
                },
            )
            .await
    }

    /// Read method
    async fn read(&mut self, id: u64, size: u64) -> Vec<u8> {
        self.handler.read(id, size).await
        // dbg!((id, size));
        // vec![0; size as usize]
    }
}

impl Audio {
    pub async fn new(conn: &zbus::Connection, #[cfg(windows)] peer_pid: u32) -> Result<Self> {
        let proxy = AudioProxy::new(conn).await?;
        Ok(Self {
            proxy,
            in_listener: None,
            out_listener: None,
            #[cfg(windows)]
            peer_pid,
        })
    }

    pub async fn register_out_listener<H: AudioOutHandler>(&mut self, handler: H) -> Result<()> {
        let (p0, p1) = UnixStream::pair()?;
        let p0 = util::prepare_uds_pass(
            #[cfg(windows)]
            self.peer_pid,
            &p0,
        )?;
        self.proxy.register_out_listener(p0).await?;
        let c = zbus::ConnectionBuilder::unix_stream(p1)
            .p2p()
            .serve_at(
                "/org/qemu/Display1/AudioOutListener",
                AudioOutListener { handler },
            )?
            .build()
            .await?;
        self.out_listener.replace(c);
        Ok(())
    }

    pub async fn register_in_listener<H: AudioInHandler>(&mut self, handler: H) -> Result<()> {
        let (p0, p1) = UnixStream::pair()?;
        let p0 = util::prepare_uds_pass(
            #[cfg(windows)]
            self.peer_pid,
            &p0,
        )?;
        self.proxy.register_in_listener(p0).await?;
        let c = zbus::ConnectionBuilder::unix_stream(p1)
            .p2p()
            .serve_at(
                "/org/qemu/Display1/AudioInListener",
                AudioInListener { handler },
            )?
            .build()
            .await?;
        self.in_listener.replace(c);
        Ok(())
    }
}
