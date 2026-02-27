use std::{
    ffi::{CStr, CString},
    io,
};

use libcamera_sys::*;

use crate::utils::handle_result;

/// Log destination type.
#[derive(Copy, Clone, Debug)]
pub enum LoggingTarget {
    None,
    Syslog,
    File,
    Stream,
}

impl From<LoggingTarget> for libcamera_logging_target_t {
    fn from(value: LoggingTarget) -> Self {
        match value {
            LoggingTarget::None => libcamera_logging_target::LIBCAMERA_LOGGING_TARGET_NONE,
            LoggingTarget::Syslog => libcamera_logging_target::LIBCAMERA_LOGGING_TARGET_SYSLOG,
            LoggingTarget::File => libcamera_logging_target::LIBCAMERA_LOGGING_TARGET_FILE,
            LoggingTarget::Stream => libcamera_logging_target::LIBCAMERA_LOGGING_TARGET_STREAM,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum LoggingLevel {
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

impl From<LoggingLevel> for &'static CStr {
    fn from(value: LoggingLevel) -> Self {
        match value {
            LoggingLevel::Debug => c"DEBUG",
            LoggingLevel::Info => c"INFO",
            LoggingLevel::Warn => c"WARN",
            LoggingLevel::Error => c"ERROR",
            LoggingLevel::Fatal => c"FATAL",
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum LoggingStream {
    StdOut,
    StdErr,
    Custom(*mut core::ffi::c_void),
}

impl From<LoggingStream> for libcamera_logging_stream_t {
    fn from(value: LoggingStream) -> Self {
        match value {
            LoggingStream::StdOut => libcamera_logging_stream::LIBCAMERA_LOGGING_STREAM_STDOUT,
            LoggingStream::StdErr => libcamera_logging_stream::LIBCAMERA_LOGGING_STREAM_STDERR,
            LoggingStream::Custom(_) => libcamera_logging_stream::LIBCAMERA_LOGGING_STREAM_CUSTOM,
        }
    }
}

/// Direct logging to a file.
pub fn log_set_file(file: &str, color: bool) -> io::Result<()> {
    let file = CString::new(file).expect("file contains null byte");
    let ret = unsafe { libcamera_log_set_file(file.as_ptr(), color) };
    handle_result(ret)
}

/// Direct logging to a stream.
pub fn log_set_stream(stream: LoggingStream, color: bool) -> io::Result<()> {
    let ret = unsafe {
        match stream {
            LoggingStream::Custom(ptr) => libcamera_log_set_custom_stream(ptr, color),
            _ => libcamera_log_set_stream(stream.into(), color),
        }
    };
    handle_result(ret)
}

/// Set the logging target.
pub fn log_set_target(target: LoggingTarget) -> io::Result<()> {
    let ret = unsafe { libcamera_log_set_target(target.into()) };
    handle_result(ret)
}

/// Convenience: direct logging to stderr and set a default level for the "Camera" category.
pub fn configure_stderr(category: &str, level: LoggingLevel, color: bool) -> io::Result<()> {
    log_set_stream(LoggingStream::StdErr, color)?;
    let cm = crate::camera_manager::CameraManager::new()?;
    cm.log_set_level(category, level);
    Ok(())
}

/// Convenience: configure logging target and stream without creating a CameraManager.
pub fn configure_logging(target: LoggingTarget, stream: Option<LoggingStream>, color: bool) -> io::Result<()> {
    if let Some(s) = stream {
        log_set_stream(s, color)?;
        log_set_target(LoggingTarget::Stream)?;
    } else {
        log_set_target(target)?;
    }
    Ok(())
}

/// Convenience: direct logging to stdout and set a default level for the given category.
pub fn configure_stdout(category: &str, level: LoggingLevel, color: bool) -> io::Result<()> {
    log_set_stream(LoggingStream::StdOut, color)?;
    set_category_level(category, level);
    Ok(())
}

/// Set the log level for a category without constructing a CameraManager.
pub fn set_category_level(category: &str, level: LoggingLevel) {
    let category = CString::new(category).expect("category contains null byte");
    let level: &CStr = level.into();
    unsafe { libcamera_log_set_level(category.as_ptr(), level.as_ptr()) };
}
