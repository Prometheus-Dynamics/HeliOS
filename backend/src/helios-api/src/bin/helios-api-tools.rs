#[path = "../api_tools_impl.rs"]
mod api_tools_impl;
#[path = "../api_tools_protocol.rs"]
mod api_tools_protocol;

use api_tools_impl::execute;
use api_tools_protocol::ApiToolsRequest;
use std::io::{Read, Write};

fn main() {
    if let Err(err) = run() {
        let _ = writeln!(std::io::stderr(), "{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input).map_err(|err| format!("failed to read stdin: {err}"))?;
    let request: ApiToolsRequest = serde_json::from_slice(&input).map_err(|err| format!("failed to decode request: {err}"))?;
    let response = execute(request)?;
    serde_json::to_writer(std::io::stdout(), &response).map_err(|err| format!("failed to encode response: {err}"))?;
    Ok(())
}
