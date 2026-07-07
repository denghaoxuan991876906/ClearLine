use crate::{ClearLineError, ClearLineResult};

pub const CLEARLINE_CONTROL_PATH: &str = r"\\.\ClearLineControl";
pub const CLEARLINE_PING_MAGIC: u32 = 0x436C_7243;
pub const CLEARLINE_PING_VERSION: u32 = 1;

const FILE_DEVICE_UNKNOWN: u32 = 0x0000_0022;
const METHOD_BUFFERED: u32 = 0;
const FILE_ANY_ACCESS: u32 = 0;
const CLEARLINE_IOCTL_PING_INDEX: u32 = 0x801;
const CLEARLINE_IOCTL_WRITE_PCM_INDEX: u32 = 0x802;
const CLEARLINE_IOCTL_GET_BUFFER_STATUS_INDEX: u32 = 0x803;
pub const IOCTL_CLEARLINE_PING: u32 = ctl_code(
    FILE_DEVICE_UNKNOWN,
    CLEARLINE_IOCTL_PING_INDEX,
    METHOD_BUFFERED,
    FILE_ANY_ACCESS,
);
pub const IOCTL_CLEARLINE_WRITE_PCM: u32 = ctl_code(
    FILE_DEVICE_UNKNOWN,
    CLEARLINE_IOCTL_WRITE_PCM_INDEX,
    METHOD_BUFFERED,
    FILE_ANY_ACCESS,
);
pub const IOCTL_CLEARLINE_GET_BUFFER_STATUS: u32 = ctl_code(
    FILE_DEVICE_UNKNOWN,
    CLEARLINE_IOCTL_GET_BUFFER_STATUS_INDEX,
    METHOD_BUFFERED,
    FILE_ANY_ACCESS,
);

const fn ctl_code(device_type: u32, function: u32, method: u32, access: u32) -> u32 {
    (device_type << 16) | (access << 14) | (function << 2) | method
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClearLinePingResponse {
    magic: u32,
    version: u32,
    sample_rate_hz: u32,
    channels: u32,
}

impl ClearLinePingResponse {
    pub fn magic(self) -> u32 {
        self.magic
    }

    pub fn version(self) -> u32 {
        self.version
    }

    pub fn sample_rate_hz(self) -> u32 {
        self.sample_rate_hz
    }

    pub fn channels(self) -> u32 {
        self.channels
    }

    pub fn validate(self) -> ClearLineResult<Self> {
        if self.magic != CLEARLINE_PING_MAGIC {
            return Err(ClearLineError::VirtualMicControl(format!(
                "unexpected ping magic 0x{:08x}",
                self.magic
            )));
        }
        if self.version != CLEARLINE_PING_VERSION {
            return Err(ClearLineError::VirtualMicControl(format!(
                "unsupported ping version {}",
                self.version
            )));
        }
        Ok(self)
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClearLineBufferStatus {
    capacity_bytes: u32,
    readable_bytes: u32,
    writable_bytes: u32,
    total_written_bytes: u64,
    total_dropped_bytes: u64,
    overflow_count: u64,
    total_read_bytes: u64,
    total_underrun_bytes: u64,
    underrun_count: u64,
}

impl ClearLineBufferStatus {
    pub fn capacity_bytes(self) -> u32 {
        self.capacity_bytes
    }

    pub fn readable_bytes(self) -> u32 {
        self.readable_bytes
    }

    pub fn writable_bytes(self) -> u32 {
        self.writable_bytes
    }

    pub fn total_written_bytes(self) -> u64 {
        self.total_written_bytes
    }

    pub fn total_dropped_bytes(self) -> u64 {
        self.total_dropped_bytes
    }

    pub fn overflow_count(self) -> u64 {
        self.overflow_count
    }

    pub fn total_read_bytes(self) -> u64 {
        self.total_read_bytes
    }

    pub fn total_underrun_bytes(self) -> u64 {
        self.total_underrun_bytes
    }

    pub fn underrun_count(self) -> u64 {
        self.underrun_count
    }
}

#[derive(Debug, Clone)]
pub struct VirtualMicControl {
    path: String,
}

impl VirtualMicControl {
    pub fn new() -> Self {
        Self::with_path(CLEARLINE_CONTROL_PATH)
    }

    pub fn with_path(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn ping(&self) -> ClearLineResult<ClearLinePingResponse> {
        platform_ping(&self.path)
    }

    pub fn write_pcm_i16_mono_48k(&self, samples: &[i16]) -> ClearLineResult<u32> {
        platform_write_pcm_i16_mono_48k(&self.path, samples)
    }

    pub fn buffer_status(&self) -> ClearLineResult<ClearLineBufferStatus> {
        platform_buffer_status(&self.path)
    }
}

impl Default for VirtualMicControl {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(windows)]
fn platform_ping(path: &str) -> ClearLineResult<ClearLinePingResponse> {
    use std::{mem, ptr};

    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::IO::DeviceIoControl;

    let handle = open_control_handle(path)?;

    let mut response = ClearLinePingResponse {
        magic: 0,
        version: 0,
        sample_rate_hz: 0,
        channels: 0,
    };
    let mut bytes_returned = 0u32;
    let ok = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_CLEARLINE_PING,
            ptr::null(),
            0,
            &mut response as *mut ClearLinePingResponse as *mut _,
            mem::size_of::<ClearLinePingResponse>() as u32,
            &mut bytes_returned,
            ptr::null_mut(),
        )
    };
    let device_io_error = if ok == 0 {
        Some(last_error("DeviceIoControl(IOCTL_CLEARLINE_PING)", path))
    } else {
        None
    };

    unsafe {
        CloseHandle(handle);
    }

    if let Some(error) = device_io_error {
        return Err(error);
    }

    if bytes_returned as usize != mem::size_of::<ClearLinePingResponse>() {
        return Err(ClearLineError::VirtualMicControl(format!(
            "ping returned {} bytes, expected {}",
            bytes_returned,
            mem::size_of::<ClearLinePingResponse>()
        )));
    }

    response.validate()
}

#[cfg(windows)]
fn platform_write_pcm_i16_mono_48k(path: &str, samples: &[i16]) -> ClearLineResult<u32> {
    use std::{mem, ptr};

    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::IO::DeviceIoControl;

    if samples.is_empty() {
        return Ok(0);
    }

    let byte_len = samples
        .len()
        .checked_mul(mem::size_of::<i16>())
        .ok_or_else(|| ClearLineError::VirtualMicControl("PCM payload is too large".into()))?;
    if byte_len > u32::MAX as usize {
        return Err(ClearLineError::VirtualMicControl(format!(
            "PCM payload has {byte_len} bytes, larger than u32::MAX"
        )));
    }

    let handle = open_control_handle(path)?;
    let mut bytes_returned = 0u32;
    let ok = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_CLEARLINE_WRITE_PCM,
            samples.as_ptr() as *mut _,
            byte_len as u32,
            ptr::null_mut(),
            0,
            &mut bytes_returned,
            ptr::null_mut(),
        )
    };
    let device_io_error = if ok == 0 {
        Some(last_error(
            "DeviceIoControl(IOCTL_CLEARLINE_WRITE_PCM)",
            path,
        ))
    } else {
        None
    };

    unsafe {
        CloseHandle(handle);
    }

    if let Some(error) = device_io_error {
        return Err(error);
    }

    Ok(byte_len as u32)
}

#[cfg(windows)]
fn platform_buffer_status(path: &str) -> ClearLineResult<ClearLineBufferStatus> {
    use std::{mem, ptr};

    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::IO::DeviceIoControl;

    let handle = open_control_handle(path)?;
    let mut status = ClearLineBufferStatus {
        capacity_bytes: 0,
        readable_bytes: 0,
        writable_bytes: 0,
        total_written_bytes: 0,
        total_dropped_bytes: 0,
        overflow_count: 0,
        total_read_bytes: 0,
        total_underrun_bytes: 0,
        underrun_count: 0,
    };
    let mut bytes_returned = 0u32;
    let ok = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_CLEARLINE_GET_BUFFER_STATUS,
            ptr::null(),
            0,
            &mut status as *mut ClearLineBufferStatus as *mut _,
            mem::size_of::<ClearLineBufferStatus>() as u32,
            &mut bytes_returned,
            ptr::null_mut(),
        )
    };
    let device_io_error = if ok == 0 {
        Some(last_error(
            "DeviceIoControl(IOCTL_CLEARLINE_GET_BUFFER_STATUS)",
            path,
        ))
    } else {
        None
    };

    unsafe {
        CloseHandle(handle);
    }

    if let Some(error) = device_io_error {
        return Err(error);
    }

    if bytes_returned as usize != mem::size_of::<ClearLineBufferStatus>() {
        return Err(ClearLineError::VirtualMicControl(format!(
            "buffer status returned {} bytes, expected {}",
            bytes_returned,
            mem::size_of::<ClearLineBufferStatus>()
        )));
    }

    Ok(status)
}

#[cfg(windows)]
fn open_control_handle(path: &str) -> ClearLineResult<windows_sys::Win32::Foundation::HANDLE> {
    use std::ptr;

    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    let wide_path = to_wide_null(path);
    let handle = unsafe {
        CreateFileW(
            wide_path.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        return Err(last_error("CreateFileW", path));
    }

    Ok(handle)
}

#[cfg(windows)]
fn last_error(operation: &str, path: &str) -> ClearLineError {
    use windows_sys::Win32::Foundation::GetLastError;

    let code = unsafe { GetLastError() };
    ClearLineError::VirtualMicControl(format!("{operation} failed for {path}: Win32 error {code}"))
}

#[cfg(windows)]
fn to_wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(not(windows))]
fn platform_ping(path: &str) -> ClearLineResult<ClearLinePingResponse> {
    Err(ClearLineError::VirtualMicControl(format!(
        "ClearLine virtual microphone control path {path} is only available on Windows"
    )))
}

#[cfg(not(windows))]
fn platform_write_pcm_i16_mono_48k(path: &str, _samples: &[i16]) -> ClearLineResult<u32> {
    Err(ClearLineError::VirtualMicControl(format!(
        "ClearLine virtual microphone control path {path} is only available on Windows"
    )))
}

#[cfg(not(windows))]
fn platform_buffer_status(path: &str) -> ClearLineResult<ClearLineBufferStatus> {
    Err(ClearLineError::VirtualMicControl(format!(
        "ClearLine virtual microphone control path {path} is only available on Windows"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_code_matches_driver_contract() {
        assert_eq!(IOCTL_CLEARLINE_PING, 0x0022_2004);
        assert_eq!(IOCTL_CLEARLINE_WRITE_PCM, 0x0022_2008);
        assert_eq!(IOCTL_CLEARLINE_GET_BUFFER_STATUS, 0x0022_200c);
    }

    #[test]
    fn ping_response_validates_magic_and_version() {
        let response = ClearLinePingResponse {
            magic: CLEARLINE_PING_MAGIC,
            version: CLEARLINE_PING_VERSION,
            sample_rate_hz: 48_000,
            channels: 1,
        };

        assert_eq!(response.validate().unwrap().sample_rate_hz(), 48_000);
    }

    #[test]
    fn ping_response_rejects_wrong_magic() {
        let response = ClearLinePingResponse {
            magic: 0,
            version: CLEARLINE_PING_VERSION,
            sample_rate_hz: 48_000,
            channels: 1,
        };

        let error = response.validate().unwrap_err();
        assert!(error.to_string().contains("unexpected ping magic"));
    }

    #[test]
    fn buffer_status_reports_available_space_and_pressure() {
        let status = ClearLineBufferStatus {
            capacity_bytes: 192_000,
            readable_bytes: 4_800,
            writable_bytes: 187_200,
            total_written_bytes: 9_600,
            total_dropped_bytes: 0,
            overflow_count: 0,
            total_read_bytes: 3_200,
            total_underrun_bytes: 1_600,
            underrun_count: 2,
        };

        assert_eq!(status.capacity_bytes(), 192_000);
        assert_eq!(status.readable_bytes(), 4_800);
        assert_eq!(status.writable_bytes(), 187_200);
        assert_eq!(status.total_written_bytes(), 9_600);
        assert_eq!(status.total_dropped_bytes(), 0);
        assert_eq!(status.overflow_count(), 0);
        assert_eq!(status.total_read_bytes(), 3_200);
        assert_eq!(status.total_underrun_bytes(), 1_600);
        assert_eq!(status.underrun_count(), 2);
    }
}
