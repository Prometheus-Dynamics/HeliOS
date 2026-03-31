#[path = "../api_tools_entry.rs"]
mod api_tools_entry;
#[path = "../api_tools_impl/mod.rs"]
mod api_tools_impl;
#[path = "../api_tools_protocol.rs"]
mod api_tools_protocol;

fn main() {
    api_tools_entry::run_process_and_exit();
}
