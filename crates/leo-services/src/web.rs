use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::{bail, Context, Result};

const TIMEOUT: Duration = Duration::from_secs(10);
const AGENT: &str = "Mozilla/5.0 (compatible; leo notes app)";

pub fn allowed(url: &reqwest::Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    match url.host() {
        Some(url::Host::Domain(name)) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            !(name == "localhost"
                || name.ends_with(".localhost")
                || name.ends_with(".local")
                || name.ends_with(".internal")
                || name.ends_with(".lan")
                || !name.contains('.'))
        }
        Some(url::Host::Ipv4(ip)) => public(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => public(IpAddr::V6(ip)),
        None => false,
    }
}

fn public_v4(v4: Ipv4Addr) -> bool {
    let [a, b, _, _] = v4.octets();
    !(a == 0
        || v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_broadcast()
        || v4.is_multicast()
        || a == 100 && (64..128).contains(&b)
        || a == 192 && b == 0 && v4.octets()[2] == 0
        || a == 198 && (18..20).contains(&b)
        || a >= 240)
}

fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => {
            let s = v6.segments();
            let wrapped = |hi: u16, lo: u16| {
                Ipv4Addr::new((hi >> 8) as u8, hi as u8, (lo >> 8) as u8, lo as u8)
            };
            if v6.is_loopback() || v6.is_unspecified() || v6.is_multicast() {
                return false;
            }
            if (s[0] & 0xfe00) == 0xfc00 || (s[0] & 0xffc0) == 0xfe80 || (s[0] & 0xffc0) == 0xfec0 {
                return false;
            }
            if let Some(v4) = v6.to_ipv4() {
                return public_v4(v4);
            }
            if s[0] == 0x0064 && s[1] == 0xff9b {
                return public_v4(wrapped(s[6], s[7]));
            }
            if s[0] == 0x2002 {
                return public_v4(wrapped(s[1], s[2]));
            }
            true
        }
    }
}

pub fn public_addresses(host: &str) -> Result<Vec<SocketAddr>> {
    let all: Vec<SocketAddr> = (host, 0)
        .to_socket_addrs()
        .with_context(|| format!("could not find {host}"))?
        .take(MOST_ADDRESSES + 1)
        .collect();
    if all.len() > MOST_ADDRESSES || all.is_empty() || !all.iter().all(|a| public(a.ip())) {
        bail!("{host} is not a public web address");
    }
    Ok(all)
}

const MOST_DNS: usize = 4;
const MOST_ADDRESSES: usize = 32;
static DNS_SLOTS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();

async fn resolve_with(
    host: String,
    slots: Arc<tokio::sync::Semaphore>,
    timeout: Duration,
    lookup: impl FnOnce(&str) -> Result<Vec<SocketAddr>> + Send + 'static,
) -> Result<Vec<SocketAddr>> {
    tokio::time::timeout(timeout, async move {
        let permit = slots
            .acquire_owned()
            .await
            .context("Web address lookup is unavailable. Try again.")?;
        let (sender, reply) = tokio::sync::oneshot::channel();
        std::thread::Builder::new()
            .name("leo-public-dns".into())
            .spawn(move || {
                let _permit = permit;
                let _ = sender.send(lookup(&host));
            })
            .context("Web address lookup could not start. Try again.")?;
        reply
            .await
            .context("Web address lookup stopped. Try again.")?
    })
    .await
    .context("Web address lookup took too long. Try again.")?
}

pub async fn public_dns(host: String) -> Result<Vec<std::net::SocketAddr>> {
    let slots =
        Arc::clone(DNS_SLOTS.get_or_init(|| Arc::new(tokio::sync::Semaphore::new(MOST_DNS))));
    resolve_with(host, slots, TIMEOUT, public_addresses).await
}
pub fn public_https(address: &str) -> bool {
    reqwest::Url::parse(address).is_ok_and(|url| url.scheme() == "https" && allowed(&url))
}

struct PublicOnly;

impl reqwest::dns::Resolve for PublicOnly {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let slots = Arc::clone(
                DNS_SLOTS.get_or_init(|| Arc::new(tokio::sync::Semaphore::new(MOST_DNS))),
            );
            resolve_with(host, slots, TIMEOUT, public_addresses)
                .await
                .map(|found| Box::new(found.into_iter()) as reqwest::dns::Addrs)
                .map_err(|e| e.into())
        })
    }
}

fn follows(url: &reqwest::Url, previous: usize, https_only: bool) -> bool {
    previous < 5 && allowed(url) && (!https_only || url.scheme() == "https")
}

fn redirect_policy(https_only: bool) -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        if follows(attempt.url(), attempt.previous().len(), https_only) {
            attempt.follow()
        } else {
            attempt.stop()
        }
    })
}

fn protected_client(
    builder: reqwest::blocking::ClientBuilder,
    https_only: bool,
    timeout: Duration,
) -> Result<reqwest::blocking::Client> {
    Ok(builder
        .no_proxy()
        .https_only(https_only)
        .timeout(timeout)
        .user_agent(AGENT)
        .dns_resolver(Arc::new(PublicOnly))
        .redirect(redirect_policy(https_only))
        .build()?)
}

#[cfg(not(test))]
pub(crate) fn model_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .user_agent(AGENT)
        .dns_resolver(Arc::new(PublicOnly))
        .redirect(redirect_policy(true))
        .build()?)
}

fn client(https_only: bool) -> Result<reqwest::blocking::Client> {
    protected_client(reqwest::blocking::Client::builder(), https_only, TIMEOUT)
}

fn streaming_client(https_only: bool, idle: Duration) -> Result<reqwest::blocking::Client> {
    protected_client(
        reqwest::blocking::Client::builder().connect_timeout(TIMEOUT),
        https_only,
        idle,
    )
}

#[derive(Debug)]
pub struct ResponseTooLarge;

impl std::fmt::Display for ResponseTooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("that address sent back more than leo will read")
    }
}

impl std::error::Error for ResponseTooLarge {}

fn copy_limited(
    mut response: reqwest::blocking::Response,
    most: u64,
    sink: &mut dyn Write,
) -> Result<()> {
    let copied = std::io::copy(&mut response.by_ref().take(most), sink)?;
    if copied == most && response.read(&mut [0])? != 0 {
        return Err(ResponseTooLarge.into());
    }
    Ok(())
}

fn limited(response: reqwest::blocking::Response, most: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    copy_limited(response, most, &mut bytes)?;
    Ok(bytes)
}

fn public_url(address: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(address).context("that is not a web address")?;
    if url.scheme() != "https" || !allowed(&url) {
        bail!("leo only opens public https addresses");
    }
    Ok(url)
}

pub fn fetch_public(address: &str, most: u64) -> Result<Vec<u8>> {
    let url = public_url(address)?;
    let response = client(true)?.get(url).send()?.error_for_status()?;
    limited(response, most)
}

pub struct Since {
    pub body: Option<Vec<u8>>,
    pub etag: Option<String>,
    pub modified: Option<String>,
}

pub struct Streamed {
    pub written: bool,
    pub etag: Option<String>,
    pub modified: Option<String>,
}

pub fn fetch_public_since(
    address: &str,
    most: u64,
    etag: Option<&str>,
    modified: Option<&str>,
) -> Result<Since> {
    let url = public_url(address)?;
    let mut body = Vec::new();
    let got = fetch_into(client(true)?.get(url), most, etag, modified, &mut body)?;
    Ok(Since {
        body: got.written.then_some(body),
        etag: got.etag,
        modified: got.modified,
    })
}

pub fn fetch_public_into(
    address: &str,
    most: u64,
    etag: Option<&str>,
    modified: Option<&str>,
    sink: &mut dyn Write,
) -> Result<Streamed> {
    let url = public_url(address)?;
    fetch_into(
        streaming_client(true, TIMEOUT)?.get(url),
        most,
        etag,
        modified,
        sink,
    )
}

fn fetch_into(
    mut request: reqwest::blocking::RequestBuilder,
    most: u64,
    etag: Option<&str>,
    modified: Option<&str>,
    sink: &mut dyn Write,
) -> Result<Streamed> {
    if let Some(tag) = etag.filter(|t| t.len() < 512) {
        request = request.header(reqwest::header::IF_NONE_MATCH, tag);
    }
    if let Some(when) = modified.filter(|t| t.len() < 128) {
        request = request.header(reqwest::header::IF_MODIFIED_SINCE, when);
    }
    let response = request.send()?.error_for_status()?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(Streamed {
            written: false,
            etag: etag.map(str::to_string),
            modified: modified.map(str::to_string),
        });
    }
    let header = |name: reqwest::header::HeaderName| {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .filter(|v| v.len() < 512)
            .map(str::to_string)
    };
    let got = Streamed {
        written: true,
        etag: header(reqwest::header::ETAG),
        modified: header(reqwest::header::LAST_MODIFIED),
    };
    copy_limited(response, most, sink)?;
    Ok(got)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_web_pages_can_be_opened() {
        let ok = |u: &str| allowed(&reqwest::Url::parse(u).unwrap());
        assert!(ok("https://en.wikipedia.org/wiki/Heap"));
        assert!(ok("http://8.8.8.8/x"));
        for bad in [
            "http://localhost:31831/api/notes",
            "http://127.0.0.1/",
            "http://192.168.1.1/",
            "http://10.0.0.5/",
            "http://169.254.169.254/latest/meta-data",
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://0.1.2.3/",
            "http://[64:ff9b::7f00:1]/",
            "http://[2002:c0a8:101::]/",
            "http://[::127.0.0.1]/",
            "http://224.0.0.1/",
            "http://printer.local/",
            "http://intranet/",
            "file:///etc/passwd",
            "ftp://example.com/x",
        ] {
            assert!(!ok(bad), "{bad}");
        }
        assert!(
            public_addresses("127.0.0.1").is_err(),
            "an address that leads to this computer is refused"
        );
        assert!(public(IpAddr::V6("2606:4700::1111".parse().unwrap())));
        assert!(
            public(IpAddr::V6("2002:0808:0808::".parse().unwrap())),
            "6to4 of a public address is fine"
        );
    }
}

#[cfg(test)]
mod boundaries {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc;

    const SERVER_WAIT: Duration = Duration::from_secs(3);
    const TEST_IDLE: Duration = Duration::from_millis(500);

    fn serve<T: Send + 'static>(
        reply: impl FnOnce(TcpStream, String) -> T + Send + 'static,
    ) -> (String, std::thread::JoinHandle<T>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let until = std::time::Instant::now() + SERVER_WAIT;
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < until =>
                    {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("the fake target did not receive a request: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream.set_read_timeout(Some(SERVER_WAIT)).unwrap();
            stream.set_write_timeout(Some(SERVER_WAIT)).unwrap();
            let mut reader = BufReader::new(&mut stream).take(4096);
            let mut request = String::new();
            while !request.ends_with("\r\n\r\n") {
                assert_ne!(reader.read_line(&mut request).unwrap(), 0);
            }
            reply(stream, request)
        });
        (url, server)
    }

    fn fetch_locally(url: &str, most: u64, idle: Duration, sink: &mut dyn Write) -> Result<()> {
        fetch_into(
            streaming_client(false, idle)?.get(url),
            most,
            None,
            None,
            sink,
        )?;
        Ok(())
    }

    #[test]
    fn a_large_body_reaches_the_sink_in_order_before_the_response_ends() {
        const PIECE: usize = 64 * 1024;
        const PIECES: &[u8] = b"abcd";
        let most = (PIECE * PIECES.len()) as u64;
        let (first_write, wait) = mpsc::channel();
        let (url, server) = serve(move |mut stream, _| {
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {most}\r\n\r\n").unwrap();
            stream.write_all(&[PIECES[0]; PIECE]).unwrap();
            let received_before_end = wait.recv_timeout(SERVER_WAIT).is_ok();
            for &byte in &PIECES[1..] {
                std::thread::sleep(TEST_IDLE / 2);
                stream.write_all(&[byte; PIECE]).unwrap();
            }
            received_before_end
        });
        struct Sink {
            bytes: Vec<u8>,
            first_write: Option<mpsc::Sender<()>>,
        }
        impl Write for Sink {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if let Some(first_write) = self.first_write.take() {
                    let _ = first_write.send(());
                }
                self.bytes.write(bytes)
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut sink = Sink {
            bytes: Vec::new(),
            first_write: Some(first_write),
        };
        fetch_locally(&url, most, TEST_IDLE, &mut sink).unwrap();
        assert!(server.join().unwrap(), "the sink must see bytes before EOF");
        assert_eq!(sink.bytes.len() as u64, most);
        let (pieces, _) = sink.bytes.as_chunks::<PIECE>();
        for (bytes, byte) in pieces.iter().zip(PIECES) {
            assert!(bytes.iter().all(|actual| actual == byte));
        }
    }

    #[test]
    fn a_stalled_body_hits_the_idle_timeout_after_delivering_its_prefix() {
        let (release, wait) = mpsc::channel();
        let (url, server) = serve(move |mut stream, _| {
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\na")
                .unwrap();
            wait.recv_timeout(SERVER_WAIT).is_ok()
        });
        let mut body = Vec::new();
        let result = fetch_locally(&url, 2, TEST_IDLE, &mut body);
        let _ = release.send(());
        assert!(
            server.join().unwrap(),
            "the client must time out before EOF"
        );
        assert!(result
            .unwrap_err()
            .downcast_ref::<std::io::Error>()
            .and_then(std::io::Error::get_ref)
            .and_then(|error| error.downcast_ref::<reqwest::Error>())
            .is_some_and(reqwest::Error::is_timeout));
        assert_eq!(body, b"a");
    }

    #[test]
    fn a_body_over_the_limit_returns_a_typed_error() {
        let (url, server) = serve(|mut stream, _| {
            stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n3\r\ndef\r\n0\r\n\r\n").unwrap();
        });
        let mut body = Vec::new();
        let error = fetch_locally(&url, 4, TEST_IDLE, &mut body).unwrap_err();
        server.join().unwrap();
        assert!(error.is::<ResponseTooLarge>());
        assert!(b"abcd".starts_with(&body));
    }

    #[test]
    fn a_streamed_request_does_not_follow_a_private_redirect() {
        let private = TcpListener::bind("127.0.0.1:0").unwrap();
        private.set_nonblocking(true).unwrap();
        let target = private.local_addr().unwrap();
        let (url, server) = serve(move |mut stream, _| {
            write!(
                stream,
                "HTTP/1.1 302 Found\r\nLocation: http://{target}/\r\nContent-Length: 0\r\n\r\n"
            )
            .unwrap();
        });
        let mut body = Vec::new();
        fetch_locally(&url, 4, TEST_IDLE, &mut body).unwrap();
        server.join().unwrap();
        assert!(body.is_empty());
        assert_eq!(
            private.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn conditional_streams_send_validators_and_leave_the_sink_empty_on_not_modified() {
        const ETAG: &str = "\"cached\"";
        const MODIFIED: &str = "Tue, 17 Nov 2026 00:00:00 GMT";
        let (url, server) = serve(|mut stream, request| {
            let request = request.to_ascii_lowercase();
            assert!(request.contains("if-none-match: \"cached\"\r\n"));
            assert!(request.contains("if-modified-since: tue, 17 nov 2026 00:00:00 gmt\r\n"));
            stream
                .write_all(b"HTTP/1.1 304 Not Modified\r\nETag: \"unused\"\r\n\r\n")
                .unwrap();
        });
        let mut body = Vec::new();
        let got = fetch_into(
            streaming_client(false, TEST_IDLE).unwrap().get(url),
            4,
            Some(ETAG),
            Some(MODIFIED),
            &mut body,
        )
        .unwrap();
        server.join().unwrap();
        assert!(!got.written);
        assert!(body.is_empty());
        assert_eq!(got.etag.as_deref(), Some(ETAG));
        assert_eq!(got.modified.as_deref(), Some(MODIFIED));
    }

    #[test]
    #[ignore = "measure peak memory with /usr/bin/time -l"]
    fn fetch_fifty_megabytes() {
        const BYTES: u64 = 50_000_000;
        let (url, server) = serve(|mut stream, _| {
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {BYTES}\r\n\r\n").unwrap();
            assert_eq!(
                std::io::copy(&mut std::io::repeat(b'a').take(BYTES), &mut stream).unwrap(),
                BYTES
            );
        });
        fetch_locally(&url, BYTES, TIMEOUT, &mut std::io::sink()).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn secure_fetches_never_follow_http_or_private_redirects() {
        let https = reqwest::Url::parse("https://example.com/calendar").unwrap();
        let http = reqwest::Url::parse("http://example.com/calendar").unwrap();
        let private = reqwest::Url::parse("https://127.0.0.1/private").unwrap();
        assert!(follows(&https, 1, true));
        assert!(!follows(&http, 1, true));
        assert!(follows(&http, 1, false));
        assert!(!follows(&private, 1, false));
        for ip in ["fec0::1", "fed0::1", "feff::1"] {
            assert!(!allowed(
                &reqwest::Url::parse(&format!("https://[{ip}]/")).unwrap()
            ));
            assert!(!public(ip.parse().unwrap()));
        }
        assert!(!follows(&https, 5, true));
        assert!(
            protected_client(reqwest::blocking::Client::builder(), true, TIMEOUT)
                .unwrap()
                .get("http://127.0.0.1:9/")
                .send()
                .is_err()
        );
    }

    #[test]
    fn the_public_fetch_client_ignores_proxies_and_bounds_response_bodies() {
        let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
        proxy.set_nonblocking(true).unwrap();
        let (url, server) = serve(|mut stream, _| {
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabcdef",
                )
                .unwrap();
        });
        let builder = reqwest::blocking::Client::builder()
            .proxy(reqwest::Proxy::all(format!("http://{}", proxy.local_addr().unwrap())).unwrap());
        let response = protected_client(builder, false, TIMEOUT)
            .unwrap()
            .get(url)
            .send()
            .unwrap();
        assert!(limited(response, 4).is_err());
        assert_eq!(
            proxy.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        server.join().unwrap();
    }

    #[test]
    fn slow_dns_has_a_deadline_and_keeps_its_slot_until_the_lookup_finishes() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let slots = Arc::new(tokio::sync::Semaphore::new(1));
        let (release, hold) = std::sync::mpsc::channel();
        let lookup_slots = Arc::clone(&slots);
        let failed = runtime.block_on(resolve_with(
            "fake.example".into(),
            lookup_slots,
            Duration::from_millis(25),
            move |_| {
                let _ = hold.recv_timeout(Duration::from_secs(2));
                Ok(vec!["8.8.8.8:0".parse().unwrap()])
            },
        ));
        let still_held = slots.available_permits() == 0;
        let other_slots = Arc::clone(&slots);
        let waiting = runtime.block_on(resolve_with(
            "fake.example".into(),
            other_slots,
            Duration::from_millis(25),
            |_| panic!("no second lookup may start while the only slot is held"),
        ));
        release.send(()).unwrap();
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(2), slots.acquire())
                .await
                .unwrap()
                .unwrap()
                .forget();
        });
        assert!(failed.is_err());
        assert!(waiting.is_err());
        assert!(still_held);
    }
}
