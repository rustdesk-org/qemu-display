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
use windows::Win32::Networking::WinSock::{WSADuplicateSocketW, SOCKET, WSAPROTOCOL_INFOW};
#[cfg(windows)]
use windows::Win32::System::Threading::PROCESS_DUP_HANDLE;

pub fn prepare_uds_pass(us: &UnixStream) -> Result<Fd> {
    #[cfg(unix)]
    {
        Ok(us.as_raw_fd().into())
    }

    #[cfg(windows)]
    {
        let pid = win32::unix_stream_get_peer_pid(us)?;
        let p = win32::ProcessHandle::open(Some(pid), PROCESS_DUP_HANDLE)?;
        let mut info = unsafe { std::mem::zeroed() };
        if unsafe {
            WSADuplicateSocketW(SOCKET(us.as_raw_socket() as _), p.process_id(), &mut info)
        } != 0
        {
            return Err(crate::Error::Io(win32::wsa_last_err()));
        }
        let info = unsafe {
            std::slice::from_raw_parts(
                &info as *const _ as *const u8,
                std::mem::size_of::<WSAPROTOCOL_INFOW>(),
            )
        };
        Ok(info.to_vec())
    }
}
