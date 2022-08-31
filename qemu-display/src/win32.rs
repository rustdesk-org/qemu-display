use std::io;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Networking::WinSock::{WSADuplicateSocketW, SOCKET, WSAPROTOCOL_INFOW};
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

    pub fn duplicate_socket(&self, sock: SOCKET) -> crate::Result<Fd> {
        let mut info = unsafe { std::mem::zeroed() };
        if unsafe { WSADuplicateSocketW(sock, self.process_id(), &mut info) } != 0 {
            return Err(crate::Error::Io(wsa_last_err()));
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

pub(crate) fn wsa_last_err() -> io::Error {
    use windows::Win32::Networking::WinSock::WSAGetLastError;

    let err = unsafe { WSAGetLastError() };
    io::Error::from_raw_os_error(err.0)
}
