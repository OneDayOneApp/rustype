use crate::config::Provider;
use crate::error::Result;

/// Build a blocking client that honours `provider.proxy` (or the standard
/// proxy environment variables). reqwest does not read macOS System
/// Settings proxies on its own, and API endpoints are frequently unreachable
/// without one.
pub fn client(provider: &Provider) -> Result<reqwest::blocking::Client> {
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(provider.timeout_secs.max(5)));
    let proxy = provider.resolved_proxy();
    if !proxy.is_empty() {
        builder = builder.proxy(reqwest::Proxy::all(&proxy)?);
    }
    Ok(builder.build()?)
}
