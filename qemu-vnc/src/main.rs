use std::error::Error;
use std::net::{TcpListener, TcpStream};
use std::{thread, time, io};
use std::sync::{Arc, Mutex};

use qemu_display_listener::{Console, Event};
use zbus::Connection;
use clap::Clap;
use vnc::{server::FramebufferUpdate, server::Event as VncEvent, PixelFormat, Rect, Server as VncServer, Error as VncError};

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


struct ServerInner {
    width: u16,
    height: u16,
}

struct Server {
    inner: Arc<Mutex<ServerInner>>,
}

impl Server {
    fn new(width: u16, height: u16) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ServerInner {
                width,
                height,
            }))
        }
    }

    fn handle_client(&self, stream: TcpStream) -> Result<(), Box<dyn Error>> {
        stream.set_read_timeout(Some(time::Duration::from_millis(100)))?;
        let (mut server, _share) = VncServer::from_tcp_stream(
            stream,
            self.inner.lock().unwrap().width,
            self.inner.lock().unwrap().height,
            PixelFormat::rgb8888(),
            "qemu-vnc experiment".into(),
        )?;
        let mut last_update: Option<time::Instant> = None;
        loop {
            let event = match server.read_event() {
                Ok(e) => e,
                Err(VncError::Io(ref e)) if e.kind() == io::ErrorKind::WouldBlock => {
                    continue;
                },
                Err(VncError::Disconnected) => {
                    return Ok(());
                }
                Err(e) => { return Err(e.into()); }
            };
            match event {
                VncEvent::FramebufferUpdateRequest { .. } => {
                    if let Some(last_update) = last_update {
                        if last_update.elapsed().as_millis() < 100 {
                            continue;
                        }
                    }
                    last_update = Some(time::Instant::now());
                    let mut fbu = FramebufferUpdate::new(&PixelFormat::rgb8888());
                    let pixel_data = vec![128; 8 * 8 * 4];
                    let rect = Rect {
                        left: 0,
                        top: 0,
                        width: 8,
                        height: 8,
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
    let (rx, ack) = console.listen()?;

    let server = Server::new(console.width()? as u16, console.height()? as u16);

    let _thread = thread::spawn(move || {
        match rx.recv().unwrap() {
            Event::Scanout(s) => {
                dbg!(&s);
                unsafe {
                    libc::close(s.fd);
                }
                let _ = ack.send(());
            },
            e => { dbg!(e); },
        }
    });
    for stream in listener.incoming() {
        server.handle_client(stream?)?;
    }
    Ok(())
}
