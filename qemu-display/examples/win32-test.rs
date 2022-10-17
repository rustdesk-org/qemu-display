#![allow(non_snake_case, non_camel_case_types)]

use std::env::args;
use std::error::Error;
use std::os::windows::io::AsRawSocket;
use std::thread::sleep;
use std::time::Duration;

use qapi::{qmp, Qmp};
use serde::{Deserialize, Serialize};
use uds_windows::UnixStream;
use windows::Win32::Networking::WinSock::{WSADuplicateSocketW, SOCKET, WSAPROTOCOL_INFOW};

use qemu_display::Display;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct get_win32_socket {
    #[serde(rename = "info")]
    pub info: ::std::string::String,
    #[serde(rename = "fdname")]
    pub fdname: ::std::string::String,
}

impl qmp::QmpCommand for get_win32_socket {}
impl qapi::Command for get_win32_socket {
    const NAME: &'static str = "get-win32-socket";
    const ALLOW_OOB: bool = false;

    type Ok = qapi::Empty;
}

fn wsa_last_err() -> std::io::Error {
    use windows::Win32::Networking::WinSock::WSAGetLastError;

    let err = unsafe { WSAGetLastError() };
    std::io::Error::from_raw_os_error(err.0)
}

// Get the process ID of the connected peer
fn unix_stream_get_peer_pid(stream: &UnixStream) -> Result<u32, std::io::Error> {
    use windows::Win32::Networking::WinSock::{WSAIoctl, IOC_OUT, IOC_VENDOR, SOCKET_ERROR};

    macro_rules! _WSAIOR {
        ($x:expr, $y:expr) => {
            IOC_OUT | $x | $y
        };
    }

    let socket = stream.as_raw_socket();
    const SIO_AF_UNIX_GETPEERPID: u32 = _WSAIOR!(IOC_VENDOR, 256);
    let mut ret = 0 as u32;
    let mut bytes = 0;

    let r = unsafe {
        WSAIoctl(
            SOCKET(socket as _),
            SIO_AF_UNIX_GETPEERPID,
            0 as *mut _,
            0,
            &mut ret as *mut _ as *mut _,
            std::mem::size_of_val(&ret) as u32,
            &mut bytes,
            0 as *mut _,
            None,
        )
    };

    if r == SOCKET_ERROR {
        return Err(wsa_last_err());
    }

    Ok(ret)
}

fn duplicate_socket(pid: u32, sock: SOCKET) -> Result<Vec<u8>, std::io::Error> {
    let mut info = unsafe { std::mem::zeroed() };
    if unsafe { WSADuplicateSocketW(sock, pid, &mut info) } != 0 {
        return Err(wsa_last_err());
    }
    let info = unsafe {
        std::slice::from_raw_parts(
            &info as *const _ as *const u8,
            std::mem::size_of::<WSAPROTOCOL_INFOW>(),
        )
    };
    Ok(info.to_vec())
}

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::init();

    let socket_addr = args().nth(1).expect("argument: QMP socket path");
    let stream = UnixStream::connect(socket_addr).expect("failed to connect to socket");
    let pid = unix_stream_get_peer_pid(&stream).expect("failed to get peer PID");

    let mut qmp = Qmp::from_stream(&stream);

    let info = qmp.handshake().expect("handshake failed");
    println!("QMP info: {:#?}", info);

    let (p0, p1) = UnixStream::pair().expect("failed to make a socketpair");
    let info =
        duplicate_socket(pid, SOCKET(p0.as_raw_socket() as _)).expect("Failed to pass socket");
    let info = base64::encode(info);
    qmp.execute(&get_win32_socket {
        info,
        fdname: "fdname".into(),
    })
    .unwrap();

    qmp.execute(&qmp::add_client {
        skipauth: None,
        tls: None,
        protocol: "@dbus-display".into(),
        fdname: "fdname".into(),
    })
    .unwrap();

    let conn = zbus::ConnectionBuilder::unix_stream(p1)
        .p2p()
        .build()
        .await
        .unwrap();

    let display = Display::new(&conn, Option::<String>::None).await.unwrap();
    loop {
        qmp.nop().unwrap();
        for event in qmp.events() {
            println!("Got event: {:#?}", event);
        }

        sleep(Duration::from_secs(1));
    }
}
