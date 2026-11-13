//! The app's HTTP clients, one per kind of peer. Each is built once and
//! reused, so connections are pooled.

use std::sync::OnceLock;
use std::time::Duration;

fn build(cell: &'static OnceLock<reqwest::Client>, make: fn() -> reqwest::ClientBuilder) -> &'static reqwest::Client {
    cell.get_or_init(|| make().build().expect("failed to build HTTP client"))
}

fn internet() -> reqwest::ClientBuilder {
    reqwest::Client::builder().user_agent(concat!("learn-polish/", env!("CARGO_PKG_VERSION")))
}

/// OpenRouter: chat, speech and the model list.
pub fn api() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    build(&CLIENT, || internet().timeout(Duration::from_secs(90)))
}

/// kie.ai and song downloads: long enough for a five-minute MP3 on a slow
/// connection.
pub fn downloads() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    build(&CLIENT, || internet().timeout(Duration::from_secs(180)))
}

/// Speakers on the home network: quick to give up, and never through a proxy.
pub fn lan() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    build(&CLIENT, || reqwest::Client::builder().timeout(Duration::from_secs(6)).no_proxy())
}
