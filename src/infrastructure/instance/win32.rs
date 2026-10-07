//! The Win32 calls the takeover needs, each behind a small safe wrapper:
//! a process handle (image name, wait, terminate), one Toolhelp lookup, the
//! TCP listener table, the service control manager and the system
//! directory. Handles close on drop.
//!
//! Nothing here loads user32 or shell32 (no `ShellExecuteExW`): a console
//! program that loads them stops receiving logoff and shutdown events.

use std::{
    ffi::OsString,
    io,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    os::windows::{
        ffi::OsStringExt,
        io::{FromRawHandle, OwnedHandle},
    },
    path::PathBuf,
    time::Duration,
};

use windows_sys::Win32::{
    Foundation::{
        ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER, ERROR_SERVICE_NOT_ACTIVE, HANDLE,
        INVALID_HANDLE_VALUE, NO_ERROR,
    },
    NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP_STATE_LISTEN, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID,
        MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
    },
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        Services::{
            CloseServiceHandle, ControlService, OpenSCManagerW, OpenServiceW, QueryServiceStatusEx,
            SC_HANDLE, SC_MANAGER_CONNECT, SC_STATUS_PROCESS_INFO, SERVICE_CONTROL_STOP,
            SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_START, SERVICE_STATUS,
            SERVICE_STATUS_PROCESS, SERVICE_STOP, SERVICE_STOPPED,
        },
        SystemInformation::GetSystemDirectoryW,
        Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, QueryFullProcessImageNameW, TerminateProcess,
            WaitForSingleObject,
        },
    },
};

/// `WAIT_OBJECT_0`: the waited-on process has ended.
const SIGNALLED: u32 = windows_sys::Win32::Foundation::WAIT_OBJECT_0;

/// Whether `error` is Windows' "access denied".
pub fn access_denied(error: &io::Error) -> bool {
    error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32)
}

/// Another process, opened once to read its image, wait for it and, if it
/// must, end it. Holding the handle keeps its process id from being reused.
pub struct ProcessHandle(OwnedHandle);

impl ProcessHandle {
    /// Opens `pid` for querying its image, waiting and terminating.
    pub fn open(pid: u32) -> io::Result<Self> {
        let access = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE;
        // SAFETY: plain call; a null result is the failure case.
        let handle = unsafe { OpenProcess(access, 0, pid) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a non-null handle OpenProcess returned, owned from here on.
        Ok(Self(unsafe { OwnedHandle::from_raw_handle(handle) }))
    }

    fn raw(&self) -> HANDLE {
        use std::os::windows::io::AsRawHandle;
        self.0.as_raw_handle()
    }

    /// The full path of the process's executable.
    pub fn image_path(&self) -> io::Result<PathBuf> {
        let mut buffer = vec![0u16; 32_768];
        let mut size = buffer.len() as u32;
        // SAFETY: the buffer holds `size` UTF-16 units; the call writes at
        // most that many and stores the length it wrote in `size`.
        let ok = unsafe {
            QueryFullProcessImageNameW(
                self.raw(),
                PROCESS_NAME_WIN32,
                buffer.as_mut_ptr(),
                &mut size,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        buffer.truncate(size as usize);
        Ok(PathBuf::from(OsString::from_wide(&buffer)))
    }

    /// Whether the process has ended (waiting at most `timeout`). A failed
    /// wait counts as still running.
    pub fn wait(&self, timeout: Duration) -> bool {
        let millis = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1);
        // SAFETY: the handle is open with SYNCHRONIZE for as long as `self`.
        unsafe { WaitForSingleObject(self.raw(), millis) == SIGNALLED }
    }

    /// Ends the process with exit code 0 (the portable launcher then closes
    /// its window without "Press Enter to close.").
    pub fn terminate(&self) -> io::Result<()> {
        // SAFETY: the handle is open with PROCESS_TERMINATE.
        if unsafe { TerminateProcess(self.raw(), 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

/// One process in a Toolhelp snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessEntry {
    pub parent: u32,
    /// The executable's file name (no folder).
    pub exe: PathBuf,
}

/// The snapshot entry of `pid`, if such a process exists. Unlike
/// `OpenProcess`, this works for a service's process too, without any right
/// on it.
pub fn process_entry(pid: u32) -> io::Result<Option<ProcessEntry>> {
    // SAFETY: plain call; INVALID_HANDLE_VALUE is the failure case.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a valid handle the snapshot call returned, owned from here on.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
    let raw = {
        use std::os::windows::io::AsRawHandle;
        snapshot.as_raw_handle()
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    // SAFETY: `entry` is a PROCESSENTRY32W with dwSize set, as both calls require.
    let mut more = unsafe { Process32FirstW(raw, &mut entry) } != 0;
    while more {
        if entry.th32ProcessID == pid {
            let length = entry
                .szExeFile
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(entry.szExeFile.len());
            return Ok(Some(ProcessEntry {
                parent: entry.th32ParentProcessID,
                exe: PathBuf::from(OsString::from_wide(&entry.szExeFile[..length])),
            }));
        }
        // SAFETY: as above.
        more = unsafe { Process32NextW(raw, &mut entry) } != 0;
    }
    Ok(None)
}

/// `%SystemRoot%\System32`, from Windows itself rather than the environment.
pub fn system_directory() -> io::Result<PathBuf> {
    let mut buffer = vec![0u16; 260];
    loop {
        // SAFETY: the buffer holds `len` UTF-16 units; the call writes at most
        // that many, or returns the size it needs.
        let written = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
        match written as usize {
            0 => return Err(io::Error::last_os_error()),
            n if n < buffer.len() => {
                buffer.truncate(n);
                return Ok(PathBuf::from(OsString::from_wide(&buffer)));
            }
            needed => buffer.resize(needed, 0),
        }
    }
}

/// The owning process of every TCP socket listening on exactly `address`,
/// from the system's listener table (no right on those processes needed).
pub fn listening_pids(address: SocketAddr) -> io::Result<Vec<u32>> {
    /// `AF_INET` and `AF_INET6` (ws2def.h).
    const AF_INET: u32 = 2;
    const AF_INET6: u32 = 23;
    let family = match address {
        SocketAddr::V4(_) => AF_INET,
        SocketAddr::V6(_) => AF_INET6,
    };
    let table = listener_table(family)?;
    let count = table.read_u32(0).unwrap_or(0) as usize;
    let mut owners = Vec::new();
    match address {
        SocketAddr::V4(asked) => {
            let start = std::mem::offset_of!(MIB_TCPTABLE_OWNER_PID, table);
            for index in 0..count {
                let Some(row) = table.read::<MIB_TCPROW_OWNER_PID>(
                    start + index * std::mem::size_of::<MIB_TCPROW_OWNER_PID>(),
                ) else {
                    break;
                };
                if row.dwState == MIB_TCP_STATE_LISTEN as u32
                    && Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes()) == *asked.ip()
                    && table_port(row.dwLocalPort) == asked.port()
                {
                    owners.push(row.dwOwningPid);
                }
            }
        }
        SocketAddr::V6(asked) => {
            let start = std::mem::offset_of!(MIB_TCP6TABLE_OWNER_PID, table);
            for index in 0..count {
                let Some(row) = table.read::<MIB_TCP6ROW_OWNER_PID>(
                    start + index * std::mem::size_of::<MIB_TCP6ROW_OWNER_PID>(),
                ) else {
                    break;
                };
                if row.dwState == MIB_TCP_STATE_LISTEN as u32
                    && Ipv6Addr::from(row.ucLocalAddr) == *asked.ip()
                    && table_port(row.dwLocalPort) == asked.port()
                {
                    owners.push(row.dwOwningPid);
                }
            }
        }
    }
    Ok(owners)
}

/// A port as the listener table stores it: in network byte order in the
/// first two bytes of a `u32`.
fn table_port(stored: u32) -> u16 {
    let bytes = stored.to_ne_bytes();
    u16::from_be_bytes([bytes[0], bytes[1]])
}

/// The bytes `GetExtendedTcpTable` wrote, kept 8-byte aligned.
struct TableBytes {
    buffer: Vec<u64>,
    /// How many bytes of `buffer` the call filled.
    len: usize,
}

impl TableBytes {
    /// A `T` at `offset`, when it lies within the bytes written.
    fn read<T: Copy>(&self, offset: usize) -> Option<T> {
        let end = offset.checked_add(std::mem::size_of::<T>())?;
        if end > self.len {
            return None;
        }
        // SAFETY: `offset..end` lies within the written part of `buffer`;
        // `T` is a plain C struct of integers, valid for any bytes, read
        // without assuming alignment.
        Some(unsafe {
            std::ptr::read_unaligned(self.buffer.as_ptr().cast::<u8>().add(offset).cast::<T>())
        })
    }

    fn read_u32(&self, offset: usize) -> Option<u32> {
        self.read::<u32>(offset)
    }
}

/// The listener table of `family`, with owning process ids. The table can
/// grow between the size query and the read, so the read is tried again.
fn listener_table(family: u32) -> io::Result<TableBytes> {
    let mut buffer: Vec<u64> = Vec::new();
    for _ in 0..8 {
        let mut size = u32::try_from(buffer.len() * 8).unwrap_or(u32::MAX);
        let pointer = if buffer.is_empty() {
            std::ptr::null_mut()
        } else {
            buffer.as_mut_ptr().cast()
        };
        // SAFETY: `pointer` is null with a size of 0, or `buffer` holding
        // `size` bytes; the call writes at most that many, or stores the size
        // it needs in `size`.
        let result = unsafe {
            GetExtendedTcpTable(
                pointer,
                &mut size,
                0,
                family,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        match result {
            NO_ERROR if !buffer.is_empty() => {
                let len = (size as usize).min(buffer.len() * 8);
                return Ok(TableBytes { buffer, len });
            }
            NO_ERROR | ERROR_INSUFFICIENT_BUFFER => {
                // Room for what it asked plus a few new listeners.
                buffer = vec![0u64; (size as usize + 1024).div_ceil(8)];
            }
            error => return Err(io::Error::from_raw_os_error(error as i32)),
        }
    }
    Err(io::Error::from_raw_os_error(
        ERROR_INSUFFICIENT_BUFFER as i32,
    ))
}

/// A service control manager or service handle, closed on drop.
struct ScHandle(SC_HANDLE);

impl Drop for ScHandle {
    fn drop(&mut self) {
        // SAFETY: a non-null handle from OpenSCManagerW/OpenServiceW, closed once.
        unsafe { CloseServiceHandle(self.0) };
    }
}

fn open_sc(handle: SC_HANDLE) -> io::Result<ScHandle> {
    if handle.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(ScHandle(handle))
    }
}

/// What the service control manager says about a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceState {
    pub running: bool,
    pub stopped: bool,
    /// The service's own process (NSSM's, not the app's); 0 when none.
    pub pid: u32,
}

/// An open service.
pub struct Service {
    // Field order: the service handle closes before its manager.
    service: ScHandle,
    _manager: ScHandle,
}

impl Service {
    /// Opens `name` to query its status only, which every interactive user may.
    pub fn open_query(name: &str) -> io::Result<Self> {
        Self::open(name, SERVICE_QUERY_STATUS)
    }

    /// Opens `name` to stop it, start it and query it. Fails with access
    /// denied (`access_denied`) unless this user was given those rights.
    pub fn open_control(name: &str) -> io::Result<Self> {
        Self::open(name, SERVICE_STOP | SERVICE_START | SERVICE_QUERY_STATUS)
    }

    fn open(name: &str, access: u32) -> io::Result<Self> {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: null machine and database names mean this computer's active
        // database; the result is checked by `open_sc`.
        let manager = open_sc(unsafe {
            OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT)
        })?;
        // SAFETY: `wide` is a NUL-terminated UTF-16 string alive for the call.
        let service = open_sc(unsafe { OpenServiceW(manager.0, wide.as_ptr(), access) })?;
        Ok(Self {
            service,
            _manager: manager,
        })
    }

    pub fn state(&self) -> io::Result<ServiceState> {
        let mut status = SERVICE_STATUS_PROCESS::default();
        let mut needed = 0u32;
        // SAFETY: the buffer is one SERVICE_STATUS_PROCESS, the size passed.
        let ok = unsafe {
            QueryServiceStatusEx(
                self.service.0,
                SC_STATUS_PROCESS_INFO,
                (&mut status as *mut SERVICE_STATUS_PROCESS).cast(),
                std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
                &mut needed,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(ServiceState {
            running: status.dwCurrentState == SERVICE_RUNNING,
            stopped: status.dwCurrentState == SERVICE_STOPPED,
            pid: status.dwProcessId,
        })
    }

    /// Asks the service to stop. A service already stopped is not an error.
    pub fn stop(&self) -> io::Result<()> {
        let mut status = SERVICE_STATUS::default();
        // SAFETY: the handle was opened with SERVICE_STOP; `status` is written.
        if unsafe { ControlService(self.service.0, SERVICE_CONTROL_STOP, &mut status) } == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_SERVICE_NOT_ACTIVE as i32) {
                return Err(error);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_reads_its_own_image_and_entry() {
        let own = std::env::current_exe().unwrap();
        let handle = ProcessHandle::open(std::process::id()).unwrap();
        assert_eq!(
            handle.image_path().unwrap().file_name(),
            own.file_name(),
            "{own:?}"
        );
        assert!(!handle.wait(Duration::ZERO));
        let entry = process_entry(std::process::id()).unwrap().unwrap();
        assert!(
            entry
                .exe
                .to_string_lossy()
                .eq_ignore_ascii_case(&own.file_name().unwrap().to_string_lossy()),
            "{entry:?}"
        );
        assert_ne!(entry.parent, 0);
        // Process ids are multiples of 4; 1 never names a process.
        assert_eq!(process_entry(1).unwrap(), None);
        assert!(ProcessHandle::open(1).is_err());
    }

    #[test]
    fn listener_table_ports_are_in_network_order() {
        // 8080 = 0x1F90, stored as the bytes 1F 90 then two unused bytes.
        assert_eq!(table_port(u32::from_ne_bytes([0x1f, 0x90, 0, 0])), 8080);
        assert_eq!(
            table_port(u32::from_ne_bytes([0x1f, 0x90, 0xab, 0xcd])),
            8080
        );
        assert_eq!(table_port(u32::from_ne_bytes([0, 80, 0, 0])), 80);
    }

    #[test]
    fn this_process_owns_its_listeners_in_the_table() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        assert_eq!(listening_pids(address).unwrap(), [std::process::id()]);
        drop(listener);
        assert!(listening_pids(address).unwrap().is_empty());
    }

    #[test]
    fn the_system_directory_holds_windows_powershell() {
        let system = system_directory().unwrap();
        assert!(system.is_absolute(), "{system:?}");
        assert!(system.join("kernel32.dll").is_file(), "{system:?}");
        let powershell = system
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        assert!(powershell.is_file(), "{powershell:?}");
    }

    #[test]
    fn a_running_service_names_its_process() {
        // The RPC service runs on every Windows; any user may query it.
        let state = Service::open_query("RpcSs").unwrap().state().unwrap();
        assert!(state.running && !state.stopped, "{state:?}");
        let entry = process_entry(state.pid).unwrap().unwrap();
        assert!(
            entry
                .exe
                .to_string_lossy()
                .eq_ignore_ascii_case("svchost.exe"),
            "{entry:?}"
        );
    }

    #[test]
    fn unknown_services_cannot_be_opened() {
        let error = Service::open_query("listening-exam-generator-no-such-service")
            .err()
            .unwrap();
        // ERROR_SERVICE_DOES_NOT_EXIST.
        assert_eq!(error.raw_os_error(), Some(1060), "{error}");
        assert!(!access_denied(&error));
        assert!(access_denied(&io::Error::from_raw_os_error(5)));
    }
}
