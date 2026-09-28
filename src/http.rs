//! HTTPS requests for the update check and `superline upgrade`, made
//! in-process with ureq and rustls so neither needs `curl` or `gh`.
//!
//! Certificates are checked against the operating system's trust store first,
//! which is what a network that intercepts TLS with its own CA needs. When
//! that store cannot be used (a minimal Linux install with no CA bundle), the
//! request is retried against the Mozilla roots bundled into the binary.
//! Proxies are read from `HTTPS_PROXY` and friends, as ureq does by default.

use std::io::{self, Read};
use std::time::Duration;

use ureq::tls::{RootCerts, TlsConfig};
use ureq::{Agent, Error};

use crate::update::CURRENT_VERSION;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Fetches `url`, following redirects, and returns its body. A response that
/// is not a success, or a body larger than `limit` bytes, is an error.
pub(crate) fn get(
    url: &str,
    headers: &[(&str, &str)],
    timeout: Duration,
    limit: u64,
) -> Result<Vec<u8>, Error> {
    download(url, headers, timeout, limit, |_| {})
}

/// Like [`get`], calling `progress` with the number of bytes received so far
/// as the body arrives.
pub(crate) fn download(
    url: &str,
    headers: &[(&str, &str)],
    timeout: Duration,
    limit: u64,
    mut progress: impl FnMut(u64),
) -> Result<Vec<u8>, Error> {
    match get_with(
        RootCerts::PlatformVerifier,
        url,
        headers,
        timeout,
        limit,
        &mut progress,
    ) {
        Err(Error::Tls(_) | Error::Rustls(_)) => get_with(
            RootCerts::WebPki,
            url,
            headers,
            timeout,
            limit,
            &mut progress,
        ),
        result => result,
    }
}

fn get_with(
    roots: RootCerts,
    url: &str,
    headers: &[(&str, &str)],
    timeout: Duration,
    limit: u64,
    progress: &mut dyn FnMut(u64),
) -> Result<Vec<u8>, Error> {
    let agent: Agent = Agent::config_builder()
        .timeout_global(Some(timeout))
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .user_agent(format!("superline/{CURRENT_VERSION}"))
        .tls_config(TlsConfig::builder().root_certs(roots).build())
        .build()
        .into();
    let mut request = agent.get(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let mut response = request.call()?;
    let mut body = Counting {
        reader: response.body_mut().with_config().limit(limit).reader(),
        received: 0,
        progress,
    };
    let mut bytes = Vec::new();
    body.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Reports how much has been read through it after every read.
struct Counting<'a, R> {
    reader: R,
    received: u64,
    progress: &'a mut dyn FnMut(u64),
}

impl<R: Read> Read for Counting<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.reader.read(buf)?;
        self.received += read as u64;
        (self.progress)(self.received);
        Ok(read)
    }
}
