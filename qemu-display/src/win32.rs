use std::io;
use uds_windows::UnixStream;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Threading::PROCESS_ACCESS_RIGHTS;

pub type Fd = Vec<u8>;

// A process handle
pub struct ProcessHandle(HANDLE);

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

impl ProcessHandle {
    // Open the process associated with the process_id (if None, the current process)
    pub fn open(
        process_id: Option<u32>,
        desired_access: PROCESS_ACCESS_RIGHTS,
    ) -> Result<Self, io::Error> {
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcess};

        let process = if let Some(process_id) = process_id {
            unsafe { OpenProcess(desired_access, false, process_id)? }
        } else {
            unsafe { GetCurrentProcess() }
        };

        Ok(Self(process))
    }

    pub fn process_id(&self) -> u32 {
        use windows::Win32::System::Threading::GetProcessId;

        unsafe { GetProcessId(self.0) }
    }
}

pub(crate) fn wsa_last_err() -> io::Error {
    use windows::Win32::Networking::WinSock::WSAGetLastError;

    let err = unsafe { WSAGetLastError() };
    io::Error::from_raw_os_error(err.0)
}

// Get the process ID of the connected peer
pub fn unix_stream_get_peer_pid(stream: &UnixStream) -> Result<u32, io::Error> {
    use std::os::windows::io::AsRawSocket;
    use windows::Win32::Networking::WinSock::{
        WSAIoctl, IOC_OUT, IOC_VENDOR, SOCKET, SOCKET_ERROR,
    };

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
