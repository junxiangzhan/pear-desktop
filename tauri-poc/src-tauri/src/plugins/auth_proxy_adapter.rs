//! Port of `src/plugins/auth-proxy-adapter/backend`: a local SOCKS5 server
//! without authentication that relays each CONNECT through the upstream
//! proxy in `options.proxy`, with its credentials. Chromium (WebView2) can't
//! pass SOCKS credentials, so with this plugin on, the webviews use the relay
//! (`proxy::init`). Desktop only: on Android the pear-gecko extension gives
//! GeckoView the credentials itself.
//!
//! Plugin backends start once the page has loaded, but the page loads
//! through the relay, so `proxy::init` already starts it at launch; the
//! plugin's start then finds it running.

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::{Arc, Mutex},
};

use serde_json::Value;
use tauri::{async_runtime::JoinHandle, AppHandle, Manager};
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use super::Plugin;
use crate::config::Config;

/// Where the relay listens: the plugin's `hostname` and `port`, with the
/// defaults of `src/plugins/auth-proxy-adapter/config.ts`.
pub fn listen_address(config: &Value) -> (String, u16) {
    let hostname = config["hostname"].as_str().filter(|h| !h.is_empty()).unwrap_or("127.0.0.1");
    let port = config["port"].as_u64().and_then(|p| u16::try_from(p).ok()).unwrap_or(4545);
    (hostname.to_owned(), port)
}

/// The authenticated upstream proxy, `socks5://user:pass@host:port`
/// (any other scheme is SOCKS4, like `parseSocksUrl`).
struct Upstream {
    host: String,
    port: u16,
    socks5: bool,
    username: String,
    password: String,
}

impl Upstream {
    fn parse(proxy: &str) -> Option<Self> {
        let url = url::Url::parse(proxy).ok()?;
        Some(Self {
            host: url.host_str()?.trim_matches(['[', ']']).to_owned(),
            port: url.port()?,
            socks5: url.scheme() == "socks5",
            username: decode(url.username()),
            password: decode(url.password().unwrap_or_default()),
        })
    }
}

/// The proxy URL without its password, for logs.
pub fn redact(proxy: &str) -> String {
    match url::Url::parse(proxy) {
        Ok(mut url) if url.password().is_some() => {
            let _ = url.set_password(Some("***"));
            url.to_string()
        }
        _ => proxy.to_owned(),
    }
}

/// `%XX` escapes in the URL's user info (`@`, `:` in credentials).
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((high * 16 + low) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

enum Target {
    Ip(IpAddr),
    Domain(String),
}

/// The running relay and its address (one per process).
static SERVER: Mutex<Option<((String, u16), JoinHandle<()>)>> = Mutex::new(None);

/// Starts the relay on `address`, unless it already runs there.
#[cfg_attr(mobile, allow(dead_code))]
pub fn ensure_running(address: (String, u16), proxy: String) {
    let mut server = SERVER.lock().unwrap();
    if server.as_ref().is_some_and(|(running, _)| *running == address) {
        return;
    }
    if let Some((_, task)) = server.take() {
        task.abort();
    }
    let upstream = Arc::new(Upstream::parse(&proxy));
    if upstream.is_none() {
        log::warn!("auth-proxy-adapter: no usable upstream proxy in options.proxy ({proxy:?})");
    }
    let (hostname, port) = address.clone();
    let task = tauri::async_runtime::spawn(async move {
            let listener = match TcpListener::bind((hostname.as_str(), port)).await {
                Ok(listener) => listener,
                Err(error) => return log::error!("auth-proxy-adapter: listening on {hostname}:{port} failed: {error}"),
            };
            log::info!("auth-proxy-adapter: SOCKS proxy at socks5://{hostname}:{port}, upstream {}", redact(&proxy));
            loop {
                let Ok((client, _)) = listener.accept().await else { continue };
                let upstream = upstream.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = relay(client, &upstream).await {
                        log::warn!("auth-proxy-adapter: {error}");
                    }
                });
            }
        });
    *server = Some((address, task));
}

#[cfg_attr(mobile, allow(dead_code))]
fn stop_server() {
    if let Some((_, task)) = SERVER.lock().unwrap().take() {
        task.abort();
    }
}

// Desktop only (see the module docs); `listen_address` is used everywhere
#[cfg_attr(mobile, allow(dead_code))]
pub struct AuthProxyAdapter;

#[cfg_attr(mobile, allow(dead_code))]
impl AuthProxyAdapter {
    fn start_server(&self, app: &AppHandle, config: &Value) {
        let proxy = crate::proxy::configured(&app.state::<Config>()).unwrap_or_default();
        ensure_running(listen_address(config), proxy);
    }
}

impl Plugin for AuthProxyAdapter {
    fn id(&self) -> &'static str {
        "auth-proxy-adapter"
    }

    fn start(&self, app: &AppHandle, config: &Value) {
        self.start_server(app, config);
    }

    fn stop(&self, _app: &AppHandle) {
        stop_server();
    }

    /// Restarts only when the address changes, like the original.
    fn on_config_change(&self, app: &AppHandle, config: &Value) {
        self.start_server(app, config);
    }
}

/// One client: the SOCKS5 handshake (no authentication, CONNECT only), then
/// the bytes both ways through the upstream proxy.
async fn relay(mut client: TcpStream, upstream: &Option<Upstream>) -> io::Result<()> {
    let mut head = [0u8; 2];
    client.read_exact(&mut head).await?;
    if head[0] != 5 {
        return Ok(()); // not SOCKS5
    }
    let mut methods = vec![0u8; head[1] as usize];
    client.read_exact(&mut methods).await?;
    if !methods.contains(&0) {
        client.write_all(&[5, 0xff]).await?;
        return Ok(());
    }
    client.write_all(&[5, 0]).await?;

    let mut request = [0u8; 4];
    client.read_exact(&mut request).await?;
    let reply = |code: u8| [5, code, 0, 1, 0, 0, 0, 0, 0, 0];
    if request[1] != 1 {
        client.write_all(&reply(7)).await?; // command not supported
        return Ok(());
    }
    let target = match request[3] {
        1 => {
            let mut ip = [0u8; 4];
            client.read_exact(&mut ip).await?;
            Target::Ip(IpAddr::V4(Ipv4Addr::from(ip)))
        }
        3 => {
            let length = client.read_u8().await? as usize;
            let mut domain = vec![0u8; length];
            client.read_exact(&mut domain).await?;
            Target::Domain(String::from_utf8_lossy(&domain).into_owned())
        }
        4 => {
            let mut ip = [0u8; 16];
            client.read_exact(&mut ip).await?;
            Target::Ip(IpAddr::V6(Ipv6Addr::from(ip)))
        }
        _ => {
            client.write_all(&reply(8)).await?; // address type not supported
            return Ok(());
        }
    };
    let port = client.read_u16().await?;

    let Some(upstream) = upstream else {
        client.write_all(&reply(1)).await?;
        return Ok(());
    };
    let mut server = match connect(upstream, &target, port).await {
        Ok(server) => server,
        Err(error) => {
            client.write_all(&reply(5)).await?; // connection refused
            return Err(error);
        }
    };
    client.write_all(&reply(0)).await?;
    // Either side closing or resetting ends the tunnel; not an error
    let _ = io::copy_bidirectional(&mut client, &mut server).await;
    Ok(())
}

/// Opens a tunnel to the target through the upstream SOCKS5 (with
/// username/password authentication when given) or SOCKS4a proxy.
async fn connect(upstream: &Upstream, target: &Target, port: u16) -> io::Result<TcpStream> {
    let fail = |what: &str| io::Error::other(format!("upstream proxy {}:{}: {what}", upstream.host, upstream.port));
    let mut server = TcpStream::connect((upstream.host.as_str(), upstream.port)).await?;
    if upstream.socks5 {
        let with_auth = !upstream.username.is_empty();
        server.write_all(if with_auth { &[5, 2, 0, 2] } else { &[5, 1, 0] }).await?;
        let mut choice = [0u8; 2];
        server.read_exact(&mut choice).await?;
        match choice[1] {
            0 => {}
            2 if with_auth => {
                let (user, pass) = (upstream.username.as_bytes(), upstream.password.as_bytes());
                if user.len() > 255 || pass.len() > 255 {
                    return Err(fail("credentials too long"));
                }
                let mut auth = vec![1, user.len() as u8];
                auth.extend_from_slice(user);
                auth.push(pass.len() as u8);
                auth.extend_from_slice(pass);
                server.write_all(&auth).await?;
                let mut status = [0u8; 2];
                server.read_exact(&mut status).await?;
                if status[1] != 0 {
                    return Err(fail("authentication failed"));
                }
            }
            _ => return Err(fail("no acceptable authentication method")),
        }
        let mut request = vec![5, 1, 0];
        match target {
            Target::Ip(IpAddr::V4(ip)) => {
                request.push(1);
                request.extend_from_slice(&ip.octets());
            }
            Target::Ip(IpAddr::V6(ip)) => {
                request.push(4);
                request.extend_from_slice(&ip.octets());
            }
            Target::Domain(domain) => {
                request.push(3);
                request.push(domain.len().min(255) as u8);
                request.extend_from_slice(&domain.as_bytes()[..domain.len().min(255)]);
            }
        }
        request.extend_from_slice(&port.to_be_bytes());
        server.write_all(&request).await?;
        let mut answer = [0u8; 4];
        server.read_exact(&mut answer).await?;
        if answer[1] != 0 {
            return Err(fail(&format!("connect refused (code {})", answer[1])));
        }
        // Skip the bound address
        let skip = match answer[3] {
            1 => 4,
            4 => 16,
            3 => server.read_u8().await? as usize,
            _ => return Err(fail("bad reply")),
        };
        let mut bound = vec![0u8; skip + 2];
        server.read_exact(&mut bound).await?;
    } else {
        // SOCKS4a: a domain goes after the user ID, with 0.0.0.1 as the address
        let mut request = vec![4, 1];
        request.extend_from_slice(&port.to_be_bytes());
        match target {
            Target::Ip(IpAddr::V4(ip)) => request.extend_from_slice(&ip.octets()),
            Target::Ip(IpAddr::V6(_)) => return Err(fail("SOCKS4 has no IPv6")),
            Target::Domain(_) => request.extend_from_slice(&[0, 0, 0, 1]),
        }
        request.extend_from_slice(upstream.username.as_bytes());
        request.push(0);
        if let Target::Domain(domain) = target {
            request.extend_from_slice(domain.as_bytes());
            request.push(0);
        }
        server.write_all(&request).await?;
        let mut answer = [0u8; 8];
        server.read_exact(&mut answer).await?;
        if answer[1] != 0x5a {
            return Err(fail(&format!("connect refused (code {:#x})", answer[1])));
        }
    }
    Ok(server)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_the_upstream_url() {
        let upstream = Upstream::parse("socks5://user%40x:p%3Ass@proxy.example:1080").unwrap();
        assert_eq!((upstream.host.as_str(), upstream.port, upstream.socks5), ("proxy.example", 1080, true));
        assert_eq!((upstream.username.as_str(), upstream.password.as_str()), ("user@x", "p:ss"));
        assert!(!Upstream::parse("socks4://h:1").unwrap().socks5);
        assert!(Upstream::parse("not a url").is_none());
        assert_eq!(redact("socks5://u:secret@h:1"), "socks5://u:***@h:1");
    }

    #[test]
    fn listen_address_defaults() {
        assert_eq!(listen_address(&json!({})), ("127.0.0.1".to_owned(), 4545));
        assert_eq!(listen_address(&json!({ "hostname": "0.0.0.0", "port": 9000 })), ("0.0.0.0".to_owned(), 9000));
    }
}
