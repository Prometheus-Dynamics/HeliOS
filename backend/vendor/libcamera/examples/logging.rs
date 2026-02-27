use libcamera::logging::{
    configure_stderr, log_set_file, log_set_stream, log_set_target, LoggingLevel, LoggingStream, LoggingTarget,
};

fn main() {
    // Disable all logging output
    log_set_target(LoggingTarget::None).expect("Can't disable logging");

    // Log to syslog
    log_set_target(LoggingTarget::Syslog).expect("Can't set logging to syslog");

    // Log to a specific file, disable color codes
    log_set_file("/tmp/libcamera.log", false).expect("Can't set logging to a file");

    // Log to stdout instead of the default stderr
    log_set_stream(LoggingStream::StdOut, true).expect("Can't set logging to stdout");

    // Convenience: log Camera category to stderr with INFO level
    configure_stderr("Camera", LoggingLevel::Info, true).expect("failed to configure stderr logging");
}
