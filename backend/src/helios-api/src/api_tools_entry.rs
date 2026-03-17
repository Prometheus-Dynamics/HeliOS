use crate::api_tools_impl::execute;
use crate::api_tools_protocol::ApiToolsRequest;
use std::io::{Read, Write};

pub(crate) fn run_process_and_exit() -> ! {
    if let Err(err) = run() {
        let _ = writeln!(std::io::stderr(), "{err}");
        std::process::exit(1);
    }
    std::process::exit(0);
}

fn run() -> Result<(), String> {
    match std::env::args().nth(1).as_deref() {
        Some("--self-check") => return Ok(()),
        Some("--help") | Some("-h") => {
            println!("usage: helios-api-tools [--self-check]");
            return Ok(());
        }
        _ => {}
    }

    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input).map_err(|err| format!("failed to read stdin: {err}"))?;
    if input.is_empty() {
        return Err("failed to decode request: empty stdin".into());
    }

    let request: ApiToolsRequest = serde_json::from_slice(&input).map_err(|err| format!("failed to decode request: {err}"))?;
    let response = execute(request)?;
    serde_json::to_writer(std::io::stdout(), &response).map_err(|err| format!("failed to encode response: {err}"))?;
    Ok(())
}
