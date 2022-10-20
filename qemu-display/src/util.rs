use crate::Result;

#[cfg(unix)]
use std::os::unix::{io::AsRawFd, net::UnixStream};
#[cfg(windows)]
use win32::Fd;
#[cfg(unix)]
use zbus::zvariant::Fd;

#[cfg(windows)]
use crate::win32;
#[cfg(windows)]
use std::os::windows::io::AsRawSocket;
#[cfg(windows)]
use uds_windows::UnixStream;
#[cfg(windows)]
use windows::Win32::Networking::WinSock::SOCKET;
#[cfg(windows)]
use windows::Win32::System::Threading::PROCESS_DUP_HANDLE;

pub fn prepare_uds_pass(#[cfg(windows)] peer_pid: u32, us: &UnixStream) -> Result<Fd> {
    #[cfg(unix)]
    {
        Ok(us.as_raw_fd().into())
    }

    #[cfg(windows)]
    {
        let p = win32::ProcessHandle::open(Some(peer_pid as _), PROCESS_DUP_HANDLE)?;
        p.duplicate_socket(SOCKET(us.as_raw_socket() as _))
    }
}
