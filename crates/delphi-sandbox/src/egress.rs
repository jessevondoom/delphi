//! The daemon-run egress proxy for sandboxed harnesses (`specs/009` §7.4).
//!
//! The OS sandbox cannot filter by hostname, so a sandboxed CLI gets exactly one network route: this
//! proxy on loopback. It speaks only `CONNECT host:port`, and a tunnel is opened only if
//!
//! 1. the target is on the allowlist (exact host name, listed port),
//! 2. the name resolves to public addresses only (no loopback, private, link-local or metadata ranges, so
//!    a name that resolves to the daemon's own sockets or the cloud metadata service is refused), and
//! 3. the TLS ClientHello that follows names the same host in its SNI (so a client cannot CONNECT to an
//!    allowed name and then talk to another).
//!
//! Every refusal is recorded, and the host and reason are all that is logged (never payloads).

use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;

const MAX_HEAD: usize = 8 * 1024;
const MAX_HELLO: usize = 16 * 1024 + 5;
const HEAD_TIMEOUT: Duration = Duration::from_secs(10);
const IDLE_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_CONNECTIONS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denied {
    pub target: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct Policy {
    /// `host` (port 443) or `host:port`, lowercase, exact. No wildcards.
    allow: HashSet<(String, u16)>,
    /// Tests only: permit loopback upstreams.
    allow_private: bool,
}

impl Policy {
    pub fn new<I: IntoIterator<Item = S>, S: AsRef<str>>(hosts: I) -> Policy {
        let allow = hosts
            .into_iter()
            .filter_map(|h| {
                let h = h.as_ref().to_ascii_lowercase();
                match h.rsplit_once(':') {
                    Some((host, port)) => Some((host.to_string(), port.parse().ok()?)),
                    None => Some((h, 443)),
                }
            })
            .collect();
        Policy { allow, allow_private: false }
    }

    /// For tests that use a loopback upstream. Never used by the daemon.
    pub fn allowing_private_addresses(mut self) -> Policy {
        self.allow_private = true;
        self
    }

    fn permits(&self, host: &str, port: u16) -> bool {
        self.allow.contains(&(host.to_ascii_lowercase(), port))
    }
}

pub struct EgressProxy {
    port: u16,
    denied: Arc<Mutex<Vec<Denied>>>,
    task: tokio::task::JoinHandle<()>,
}

impl EgressProxy {
    /// Listen on an ephemeral loopback port.
    pub async fn start(policy: Policy) -> std::io::Result<EgressProxy> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let denied = Arc::new(Mutex::new(Vec::new()));
        let (policy, log, permits) = (Arc::new(policy), denied.clone(), Arc::new(Semaphore::new(MAX_CONNECTIONS)));
        let task = tokio::spawn(async move {
            loop {
                let Ok((conn, peer)) = listener.accept().await else { break };
                if !peer.ip().is_loopback() {
                    continue;
                }
                let Ok(permit) = permits.clone().try_acquire_owned() else { continue };
                let (policy, log) = (policy.clone(), log.clone());
                tokio::spawn(async move {
                    if let Err(d) = serve(conn, &policy).await {
                        log.lock().unwrap().push(d);
                    }
                    drop(permit);
                });
            }
        });
        Ok(EgressProxy { port, denied, task })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Everything refused so far.
    pub fn denied(&self) -> Vec<Denied> {
        self.denied.lock().unwrap().clone()
    }
}

impl Drop for EgressProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn deny(target: impl Into<String>, reason: impl Into<String>) -> Denied {
    Denied { target: target.into(), reason: reason.into() }
}

/// Read up to the end of the request head.
async fn read_head(conn: &mut TcpStream) -> Result<Vec<u8>, Denied> {
    let mut head = Vec::with_capacity(512);
    let mut buf = [0u8; 512];
    loop {
        if head.windows(4).any(|w| w == b"\r\n\r\n") {
            return Ok(head);
        }
        if head.len() > MAX_HEAD {
            return Err(deny("?", "request head too large"));
        }
        let n = match tokio::time::timeout(HEAD_TIMEOUT, conn.read(&mut buf)).await {
            Ok(Ok(n)) if n > 0 => n,
            _ => return Err(deny("?", "no complete request")),
        };
        head.extend_from_slice(&buf[..n]);
    }
}

/// `CONNECT host:port HTTP/1.x` -> (host, port). Anything else is refused.
fn parse_connect(head: &[u8]) -> Result<(String, u16), Denied> {
    let line = head.split(|b| *b == b'\r').next().unwrap_or_default();
    let line = std::str::from_utf8(line).map_err(|_| deny("?", "not text"))?;
    let mut parts = line.split(' ');
    let (method, target, version) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    if method != "CONNECT" || !version.starts_with("HTTP/1.") || parts.next().is_some() {
        return Err(deny(target, format!("only CONNECT is allowed (got `{}`)", method.chars().take(16).collect::<String>())));
    }
    let (host, port) = target.rsplit_once(':').ok_or_else(|| deny(target, "no port"))?;
    let port: u16 = port.parse().map_err(|_| deny(target, "bad port"))?;
    let ok = !host.is_empty() && host.len() <= 253 && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    if !ok {
        return Err(deny(target, "bad host"));
    }
    Ok((host.to_ascii_lowercase(), port))
}

fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            !(v.is_loopback() || v.is_private() || v.is_link_local() || v.is_unspecified() || v.is_broadcast() || v.is_multicast() || o[0] == 0 || (o[0] == 100 && (64..128).contains(&o[1])) || (o[0] == 192 && o[1] == 0 && o[2] == 0))
        }
        IpAddr::V6(v) => {
            if let Some(v4) = v.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let s = v.segments();
            !(v.is_loopback() || v.is_unspecified() || v.is_multicast() || (s[0] & 0xfe00) == 0xfc00 || (s[0] & 0xffc0) == 0xfe80)
        }
    }
}

async fn resolve(host: &str, port: u16, policy: &Policy) -> Result<Vec<SocketAddr>, Denied> {
    let target = format!("{}:{}", host, port);
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port)).await.map_err(|_| deny(&target, "does not resolve"))?.collect();
    if addrs.is_empty() {
        return Err(deny(&target, "does not resolve"));
    }
    // Every address must be acceptable: a name that also points at a private range is refused outright.
    if !policy.allow_private && addrs.iter().any(|a| !is_public(a.ip())) {
        return Err(deny(&target, "resolves to a private, loopback or link-local address"));
    }
    Ok(addrs)
}

/// The server name in a TLS ClientHello record, if it has one.
pub fn sni_of(record: &[u8]) -> Option<String> {
    // record header: type 22, version, length; handshake: type 1, length(3), version(2), random(32)
    let mut p = 0usize;
    let take = |p: &mut usize, n: usize| -> Option<&[u8]> {
        let s = record.get(*p..*p + n)?;
        *p += n;
        Some(s)
    };
    let hdr = take(&mut p, 5)?;
    if hdr[0] != 22 {
        return None;
    }
    let hs = take(&mut p, 4)?;
    if hs[0] != 1 {
        return None;
    }
    take(&mut p, 2 + 32)?;
    let sid = take(&mut p, 1)?[0] as usize;
    take(&mut p, sid)?;
    let cs = take(&mut p, 2)?;
    take(&mut p, u16::from_be_bytes([cs[0], cs[1]]) as usize)?;
    let cm = take(&mut p, 1)?[0] as usize;
    take(&mut p, cm)?;
    let el = take(&mut p, 2)?;
    let ext_end = p + u16::from_be_bytes([el[0], el[1]]) as usize;
    while p + 4 <= ext_end {
        let h = take(&mut p, 4)?;
        let (ty, len) = (u16::from_be_bytes([h[0], h[1]]), u16::from_be_bytes([h[2], h[3]]) as usize);
        let body = take(&mut p, len)?;
        if ty == 0 {
            // server_name_list: len(2), then entries: type(1)=0, len(2), name
            let list = body.get(2..)?;
            if list.first() == Some(&0) {
                let n = u16::from_be_bytes([*list.get(1)?, *list.get(2)?]) as usize;
                return std::str::from_utf8(list.get(3..3 + n)?).ok().map(|s| s.to_ascii_lowercase());
            }
        }
    }
    None
}

async fn serve(mut client: TcpStream, policy: &Policy) -> Result<(), Denied> {
    let head = read_head(&mut client).await;
    let head = match head {
        Ok(h) => h,
        Err(d) => {
            let _ = client.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
            return Err(d);
        }
    };
    let (host, port) = match parse_connect(&head) {
        Ok(t) => t,
        Err(d) => {
            let _ = client.write_all(b"HTTP/1.1 405 Method Not Allowed\r\n\r\n").await;
            return Err(d);
        }
    };
    let target = format!("{}:{}", host, port);
    if !policy.permits(&host, port) {
        let _ = client.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await;
        return Err(deny(target, "host is not on the allowlist"));
    }
    let addrs = match resolve(&host, port, policy).await {
        Ok(a) => a,
        Err(d) => {
            let _ = client.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await;
            return Err(d);
        }
    };
    // Anything the client sent after the head belongs to the tunnel; with CONNECT it sends nothing until 200.
    client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await.map_err(|e| deny(&target, e.to_string()))?;
    // The SNI check: read the first TLS record and require it to name the CONNECT host.
    let mut hello = vec![0u8; MAX_HELLO];
    let mut got = 0usize;
    loop {
        let n = match tokio::time::timeout(HEAD_TIMEOUT, client.read(&mut hello[got..])).await {
            Ok(Ok(n)) if n > 0 => n,
            _ => return Err(deny(target, "no TLS ClientHello")),
        };
        got += n;
        if got >= 5 {
            let need = 5 + u16::from_be_bytes([hello[3], hello[4]]) as usize;
            if need > MAX_HELLO {
                return Err(deny(target, "oversized TLS record"));
            }
            if got >= need {
                break;
            }
        }
    }
    match sni_of(&hello[..got]) {
        Some(name) if name == host => {}
        Some(name) => return Err(deny(target, format!("SNI `{}` does not match the CONNECT host", name))),
        None => return Err(deny(target, "no SNI in the ClientHello")),
    }
    // Each resolved address in turn (an IPv6 address may be listed first and unreachable).
    let mut upstream = None;
    let mut last = String::from("no address");
    for addr in addrs {
        match tokio::time::timeout(HEAD_TIMEOUT, TcpStream::connect(addr)).await {
            Ok(Ok(s)) => {
                upstream = Some(s);
                break;
            }
            Ok(Err(e)) => last = e.to_string(),
            Err(_) => last = "connect timed out".into(),
        }
    }
    let mut upstream = upstream.ok_or_else(|| deny(&target, last))?;
    upstream.write_all(&hello[..got]).await.map_err(|e| deny(&target, e.to_string()))?;
    // Tunnel until either side closes or the connection goes idle.
    let (mut cr, mut cw) = client.split();
    let (mut ur, mut uw) = upstream.split();
    let up = async {
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            match tokio::time::timeout(IDLE_TIMEOUT, cr.read(&mut buf)).await {
                Ok(Ok(n)) if n > 0 => uw.write_all(&buf[..n]).await?,
                _ => return std::io::Result::Ok(()),
            }
        }
    };
    let down = async {
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            match tokio::time::timeout(IDLE_TIMEOUT, ur.read(&mut buf)).await {
                Ok(Ok(n)) if n > 0 => cw.write_all(&buf[..n]).await?,
                _ => return std::io::Result::Ok(()),
            }
        }
    };
    let _ = tokio::select! { r = up => r, r = down => r };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A syntactically valid TLS ClientHello record naming `sni` (or none).
    fn client_hello(sni: Option<&str>) -> Vec<u8> {
        let mut ext = Vec::new();
        if let Some(n) = sni {
            let name = n.as_bytes();
            let mut list = vec![0u8];
            list.extend((name.len() as u16).to_be_bytes());
            list.extend(name);
            let mut body = (list.len() as u16).to_be_bytes().to_vec();
            body.extend(list);
            ext.extend([0u8, 0]);
            ext.extend((body.len() as u16).to_be_bytes());
            ext.extend(body);
        }
        let mut hs = vec![3u8, 3];
        hs.extend([7u8; 32]);
        hs.push(0); // session id
        hs.extend([0u8, 2, 0x13, 0x01]); // one cipher suite
        hs.extend([1u8, 0]); // null compression
        hs.extend((ext.len() as u16).to_be_bytes());
        hs.extend(ext);
        let mut m = vec![1u8];
        m.extend(&(hs.len() as u32).to_be_bytes()[1..]);
        m.extend(hs);
        let mut rec = vec![22u8, 3, 1];
        rec.extend((m.len() as u16).to_be_bytes());
        rec.extend(m);
        rec
    }

    async fn upstream() -> (u16, tokio::task::JoinHandle<Vec<u8>>) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let h = tokio::spawn(async move {
            let (mut c, _) = l.accept().await.unwrap();
            let mut got = vec![0u8; 4096];
            let n = c.read(&mut got).await.unwrap();
            c.write_all(b"pong").await.unwrap();
            got.truncate(n);
            got
        });
        (port, h)
    }

    async fn connect(proxy: u16, request: &[u8]) -> (String, TcpStream) {
        let mut c = TcpStream::connect(("127.0.0.1", proxy)).await.unwrap();
        c.write_all(request).await.unwrap();
        let mut buf = [0u8; 256];
        let n = tokio::time::timeout(Duration::from_secs(3), c.read(&mut buf)).await.unwrap().unwrap();
        (String::from_utf8_lossy(&buf[..n]).into_owned(), c)
    }

    #[test]
    fn sni_is_read_from_a_client_hello() {
        assert_eq!(sni_of(&client_hello(Some("API.Anthropic.com"))).as_deref(), Some("api.anthropic.com"));
        assert_eq!(sni_of(&client_hello(None)), None);
        assert_eq!(sni_of(b"GET / HTTP/1.1\r\n\r\n"), None);
        assert_eq!(sni_of(&client_hello(Some("x.com"))[..20]), None, "a truncated record is not parsed");
    }

    #[test]
    fn public_addresses_only() {
        for bad in ["127.0.0.1", "10.1.2.3", "192.168.0.1", "172.16.0.1", "169.254.169.254", "0.0.0.0", "100.64.0.1", "::1", "fe80::1", "fc00::1", "::ffff:127.0.0.1"] {
            assert!(!is_public(bad.parse().unwrap()), "{}", bad);
        }
        for good in ["8.8.8.8", "160.79.104.10", "2606:4700::1111"] {
            assert!(is_public(good.parse().unwrap()), "{}", good);
        }
    }

    #[tokio::test]
    async fn an_allowed_host_with_a_matching_sni_is_tunnelled() {
        let (up, got) = upstream().await;
        let proxy = EgressProxy::start(Policy::new([format!("localhost:{}", up)]).allowing_private_addresses()).await.unwrap();
        let (reply, mut c) = connect(proxy.port(), format!("CONNECT localhost:{} HTTP/1.1\r\nHost: x\r\n\r\n", up).as_bytes()).await;
        assert!(reply.starts_with("HTTP/1.1 200"), "{}", reply);
        let hello = client_hello(Some("localhost"));
        c.write_all(&hello).await.unwrap();
        let mut buf = [0u8; 16];
        let n = tokio::time::timeout(Duration::from_secs(3), c.read(&mut buf)).await.unwrap().unwrap();
        assert_eq!(&buf[..n], b"pong");
        assert_eq!(got.await.unwrap(), hello, "the hello reaches the upstream unchanged");
        assert!(proxy.denied().is_empty());
    }

    #[tokio::test]
    async fn everything_else_is_refused_and_logged() {
        let (up, _h) = upstream().await;
        let proxy = EgressProxy::start(Policy::new([format!("localhost:{}", up)]).allowing_private_addresses()).await.unwrap();
        // not on the allowlist
        let (r, _) = connect(proxy.port(), b"CONNECT evil.example:443 HTTP/1.1\r\n\r\n").await;
        assert!(r.starts_with("HTTP/1.1 403"), "{}", r);
        // right host, wrong port
        let (r, _) = connect(proxy.port(), b"CONNECT localhost:1 HTTP/1.1\r\n\r\n").await;
        assert!(r.starts_with("HTTP/1.1 403"), "{}", r);
        // plain HTTP through the proxy
        let (r, _) = connect(proxy.port(), b"GET http://localhost/ HTTP/1.1\r\n\r\n").await;
        assert!(r.starts_with("HTTP/1.1 405"), "{}", r);
        // an allowed CONNECT whose TLS hello names another host
        let (r, mut c) = connect(proxy.port(), format!("CONNECT localhost:{} HTTP/1.1\r\n\r\n", up).as_bytes()).await;
        assert!(r.starts_with("HTTP/1.1 200"));
        c.write_all(&client_hello(Some("evil.example"))).await.unwrap();
        let mut b = [0u8; 8];
        assert_eq!(tokio::time::timeout(Duration::from_secs(3), c.read(&mut b)).await.unwrap().unwrap(), 0, "the tunnel is closed");
        // a hello with no SNI at all
        let (_, mut c) = connect(proxy.port(), format!("CONNECT localhost:{} HTTP/1.1\r\n\r\n", up).as_bytes()).await;
        c.write_all(&client_hello(None)).await.unwrap();
        assert_eq!(tokio::time::timeout(Duration::from_secs(3), c.read(&mut b)).await.unwrap().unwrap(), 0);
        tokio::time::sleep(Duration::from_millis(100)).await;
        let reasons: Vec<String> = proxy.denied().into_iter().map(|d| d.reason).collect();
        assert!(reasons.iter().any(|r| r.contains("allowlist")) && reasons.iter().any(|r| r.contains("only CONNECT")) && reasons.iter().any(|r| r.contains("does not match")) && reasons.iter().any(|r| r.contains("no SNI")), "{:?}", reasons);
    }

    #[tokio::test]
    async fn an_allowed_name_that_resolves_to_a_private_address_is_refused() {
        // The default policy: `localhost` is allowlisted but resolves to loopback.
        let proxy = EgressProxy::start(Policy::new(["localhost:443"])).await.unwrap();
        let (r, _) = connect(proxy.port(), b"CONNECT localhost:443 HTTP/1.1\r\n\r\n").await;
        assert!(r.starts_with("HTTP/1.1 403"), "{}", r);
        assert!(proxy.denied()[0].reason.contains("private"), "{:?}", proxy.denied());
    }

    #[test]
    fn malformed_requests_are_refused() {
        for bad in ["CONNECT /x HTTP/1.1", "CONNECT a:b HTTP/1.1", "CONNECT bad host:443 HTTP/1.1", "CONNECT a:443", "CONNECT a:443 HTTP/2", "CONNECT :443 HTTP/1.1", "CONNECT a$b:443 HTTP/1.1"] {
            assert!(parse_connect(format!("{}\r\n\r\n", bad).as_bytes()).is_err(), "{}", bad);
        }
        assert_eq!(parse_connect(b"CONNECT API.Example.com:443 HTTP/1.1\r\n\r\n").unwrap(), ("api.example.com".to_string(), 443));
    }
}
