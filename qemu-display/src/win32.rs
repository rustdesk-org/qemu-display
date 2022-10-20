use std::io;
use std::os::windows::io::AsRawSocket;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Networking::WinSock::{WSADuplicateSocketW, SOCKET, WSAPROTOCOL_INFOW};
use windows::Win32::System::Threading::PROCESS_ACCESS_RIGHTS;

#[cfg(feature = "qmp")]
use uds_windows::UnixStream;

pub type Fd = Vec<u8>;

// A process handle
pub struct ProcessHandle(HANDLE);

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

pub(crate) fn duplicate_socket(pid: u32, sock: SOCKET) -> crate::Result<Vec<u8>> {
    let mut info = unsafe { std::mem::zeroed() };
    if unsafe { WSADuplicateSocketW(sock, pid, &mut info) } != 0 {
        return Err(wsa_last_err().into());
    }
    let info = unsafe {
        std::slice::from_raw_parts(
            &info as *const _ as *const u8,
            std::mem::size_of::<WSAPROTOCOL_INFOW>(),
        )
    };
    Ok(info.to_vec())
}

impl ProcessHandle {
    // Open the process associated with the process_id (if None, the current process)
    pub fn open(
        process_id: Option<u32>,
        desired_access: PROCESS_ACCESS_RIGHTS,
    ) -> Result<Self, io::Error> {
        use windows::Win32::System::Threading::{
            GetCurrentProcess, OpenProcess, PROCESS_QUERY_INFORMATION,
        };

        let process = if let Some(process_id) = process_id {
            let desired_access = desired_access | PROCESS_QUERY_INFORMATION;
            unsafe { OpenProcess(desired_access, false, process_id)? }
        } else {
            unsafe { GetCurrentProcess() }
        };

        Ok(Self(process))
    }

    pub fn process_id(&self) -> crate::Result<u32> {
        use windows::Win32::Foundation::GetLastError;
        use windows::Win32::System::Threading::GetProcessId;

        unsafe {
            let pid = GetProcessId(self.0);
            if pid == 0 {
                Err(io::Error::from_raw_os_error(GetLastError().0 as _).into())
            } else {
                Ok(pid)
            }
        }
    }

    pub fn duplicate_socket(&self, sock: SOCKET) -> crate::Result<Fd> {
        duplicate_socket(self.process_id()?, sock)
    }
}

pub(crate) fn wsa_last_err() -> io::Error {
    use windows::Win32::Networking::WinSock::WSAGetLastError;

    let err = unsafe { WSAGetLastError() };
    io::Error::from_raw_os_error(err.0)
}

// Get the process ID of the connected peer
#[cfg(feature = "qmp")]
pub(crate) fn unix_stream_get_peer_pid(stream: &UnixStream) -> Result<u32, std::io::Error> {
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
