//! `nit --version` — the canonical "is nit up / installed" check. Prints the
//! client's build, then the server's (a 1-second probe of `/api/health`), and
//! exits non-zero when the server can't be reached.

use std::io::Write;

use super::client;

pub fn version() {
    println!("client {}", crate::VERSION);
    if let Some(server) = client::server_version(&client::server_url(None)) {
        println!("server {server}");
    } else {
        println!("server unreachable");
        // Flush before the abrupt exit — destructors don't run.
        std::io::stdout().flush().ok();
        std::process::exit(1);
    }
}
