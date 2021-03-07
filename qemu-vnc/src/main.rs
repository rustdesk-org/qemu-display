use std::error::Error;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::{io, thread, time};

use clap::Clap;
use image::GenericImage;
use qemu_display_listener::{Console, ConsoleEvent};
use vnc::{
    server::Event as VncEvent, server::FramebufferUpdate, Error as VncError, PixelFormat, Rect,
    Server as VncServer,
};
use zbus::Connection;

#[derive(Clap, Debug)]
pub struct SocketAddrArgs {
    /// IP address
    #[clap(short, long, default_value = "127.0.0.1")]
    address: std::net::IpAddr,
    /// IP port number
    #[clap(short, long, default_value = "5900")]
    port: u16,
}

impl From<SocketAddrArgs> for std::net::SocketAddr {
    fn from(args: SocketAddrArgs) -> Self {
        (args.address, args.port).into()
    }
}

#[derive(Clap, Debug)]
struct Cli {
    #[clap(flatten)]
    address: SocketAddrArgs,
}

const PIXMAN_X8R8G8B8: u32 = 0x20020888;
type BgraImage = image::ImageBuffer<image::Bgra<u8>, Vec<u8>>;

#[derive(Debug)]
struct ServerInner {
    image: BgraImage,
}

#[derive(Clone, Debug)]
struct Server {
    inner: Arc<Mutex<ServerInner>>,
}

impl Server {
    fn new(width: u16, height: u16) -> Self {
        let image = BgraImage::new(width as _, height as _);
        Self {
            inner: Arc::new(Mutex::new(ServerInner { image })),
        }
    }

    fn width_height(&self) -> (u16, u16) {
        let inner = self.inner.lock().unwrap();
        (inner.image.width() as u16, inner.image.height() as u16)
    }

    fn handle_client(&self, stream: TcpStream) -> Result<(), Box<dyn Error>> {
        stream.set_read_timeout(Some(time::Duration::from_millis(100)))?;
        let (width, height) = self.width_height();
        let (mut server, _share) = VncServer::from_tcp_stream(
            stream,
            width,
            height,
            PixelFormat::rgb8888(),
            "qemu-vnc experiment".into(),
        )?;
        let mut last_update: Option<time::Instant> = None;
        loop {
            let event = match server.read_event() {
                Ok(e) => e,
                Err(VncError::Io(ref e)) if e.kind() == io::ErrorKind::WouldBlock => {
                    continue;
                }
                Err(VncError::Disconnected) => {
                    return Ok(());
                }
                Err(e) => {
                    return Err(e.into());
                }
            };
            match event {
                VncEvent::FramebufferUpdateRequest { .. } => {
                    if let Some(last_update) = last_update {
                        if last_update.elapsed().as_millis() < 100 {
                            continue;
                        }
                    }
                    last_update = Some(time::Instant::now());
                    let inner = self.inner.lock().unwrap();
                    let mut fbu = FramebufferUpdate::new(&PixelFormat::rgb8888());
                    let pixel_data = inner.image.as_raw();
                    let rect = Rect {
                        left: 0,
                        top: 0,
                        width: inner.image.width() as u16,
                        height: inner.image.height() as u16,
                    };
                    fbu.add_raw_pixels(rect, &pixel_data);
                    server.send(&fbu)?;
                }
                event => {
                    dbg!(event);
                }
            }
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();

    let listener = TcpListener::bind::<std::net::SocketAddr>(args.address.into()).unwrap();
    let conn = Connection::new_session().expect("Failed to connect to DBus");
    let console = Console::new(&conn, 0).expect("Failed to get the console");
    let (rx, _ack) = console.listen()?;

    let server = Server::new(console.width()? as u16, console.height()? as u16);

    let srv = server.clone();
    let _thread = thread::spawn(move || loop {
        match rx.recv().unwrap() {
            ConsoleEvent::ScanoutDMABUF(_) => {
                unimplemented!();
            }
            ConsoleEvent::Scanout(s) => {
                if s.format != PIXMAN_X8R8G8B8 {
                    todo!()
                }
                let layout = image::flat::SampleLayout {
                    channels: 4,
                    channel_stride: 1,
                    width: s.width,
                    width_stride: 4,
                    height: s.height,
                    height_stride: s.stride as _,
                };
                let samples = image::flat::FlatSamples {
                    samples: s.data,
                    layout,
                    color_hint: None,
                };
                let img = match samples.try_into_buffer::<image::Bgra<u8>>() {
                    Ok(buf) => buf,
                    Err((_, samples)) => {
                        let view = samples.as_view::<image::Bgra<u8>>().unwrap();
                        let mut img = BgraImage::new(s.width, s.height);
                        img.copy_from(&view, 0, 0).unwrap();
                        img
                    }
                };
                let mut inner = srv.inner.lock().unwrap();
                inner.image = img;
            }
            e => {
                dbg!(e);
            }
        }
    });
    for stream in listener.incoming() {
        server.handle_client(stream?)?;
    }
    Ok(())
}
