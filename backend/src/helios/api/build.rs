//! Stamps each build with `HELIOS_BUILD_ID`, which the event stream's `hello` carries: a UI left open
//! across an update sees another build on its next connection and offers to reload.

use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    println!("cargo:rerun-if-changed=src");
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_nanos()).unwrap_or_default();
    println!("cargo:rustc-env=HELIOS_BUILD_ID={stamp:x}");
}
