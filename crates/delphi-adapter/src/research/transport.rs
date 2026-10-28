//! The one place the research job touches the network (`specs/028` §10.2): a blocking GET that never follows a redirect on its own
//! (the fetcher checks every hop against the trusted lists), caps the body, and never puts a URL or a header in an error, since a
//! search request carries the provider's key.

use std::io::Read;
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct Response {
    pub status: u16,
    pub location: Option<String>,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    /// The body was longer than the cap and was cut.
    pub truncated: bool,
}

pub trait Transport: Send + Sync {
    /// GET `url`. `Err` is a network failure (no connection, timeout, bad TLS); an HTTP error status is a normal `Response`.
    fn get(&self, url: &str, headers: &[(&str, &str)], max_bytes: usize, timeout: Duration) -> Result<Response, String>;
}

/// The real transport. Blocking, so the job runs it on a blocking thread.
pub struct HttpTransport {
    client: reqwest::blocking::Client,
}

impl HttpTransport {
    pub fn new() -> Result<HttpTransport, String> {
        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("delphi/", env!("CARGO_PKG_VERSION"), " (research; +https://github.com/jessevondoom/delphi)"))
            .build()
            .map_err(|_| "could not set up the HTTP client".to_string())?;
        Ok(HttpTransport { client })
    }
}

impl Transport for HttpTransport {
    fn get(&self, url: &str, headers: &[(&str, &str)], max_bytes: usize, timeout: Duration) -> Result<Response, String> {
        let mut req = self.client.get(url).timeout(timeout);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = req.send().map_err(|e| if e.is_timeout() { "timed out".to_string() } else if e.is_connect() { "could not connect".to_string() } else { "the request failed".to_string() })?;
        let status = resp.status().as_u16();
        let h = |n: &str| resp.headers().get(n).and_then(|v| v.to_str().ok()).map(str::to_string);
        let (location, content_type) = (h("location"), h("content-type"));
        let mut body = Vec::new();
        let n = resp.take(max_bytes as u64 + 1).read_to_end(&mut body).map_err(|_| "the response could not be read".to_string())?;
        let truncated = n > max_bytes;
        body.truncate(max_bytes);
        Ok(Response { status, location, content_type, body, truncated })
    }
}
