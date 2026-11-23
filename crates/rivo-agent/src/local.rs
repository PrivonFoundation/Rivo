//! Discovery of local (and same-LAN) model servers.
//!
//! Most local runtimes expose an OpenAI-compatible `/v1/models` endpoint, so Rivo can find a
//! server by probing its standard port and then list its models without an API key. The scan
//! covers loopback by default; optionally it also walks the local /24 subnet (the usual home
//! Wi-Fi network) and/or one extra host the user types in.
//!
//! Everything here is synchronous and free of UI concerns: `scan` is meant to run on a worker
//! thread. Port probing uses short TCP connect timeouts and a bounded worker pool, so even a
//! full subnet scan finishes in seconds and never hangs the app.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::Value;

/// A model server Rivo knows how to talk to. The port is the one the tool listens on by
/// default; `api` is the prefix of its OpenAI-compatible REST API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackendSpec {
    pub name: &'static str,
    pub ports: &'static [u16],
    pub api: &'static str,
}

/// The local-model runtimes covered by discovery. Together they represent the usual tools people
/// use to serve weights on their own machine: all of them speak OpenAI-compatible HTTP (Ollama
/// also has a native `/api/tags` endpoint, which we use as a fallback).
pub const BACKENDS: &[BackendSpec] = &[
    BackendSpec { name: "Ollama", ports: &[11434], api: "/v1" },
    BackendSpec { name: "LM Studio", ports: &[1234], api: "/v1" },
    BackendSpec { name: "llama.cpp", ports: &[8080], api: "/v1" },
    BackendSpec { name: "vLLM", ports: &[8000], api: "/v1" },
    BackendSpec { name: "LocalAI", ports: &[8081], api: "/v1" },
    BackendSpec { name: "text-generation-webui", ports: &[5000], api: "/v1" },
    BackendSpec { name: "Jan", ports: &[1337], api: "/v1" },
    BackendSpec { name: "GPT4All", ports: &[4891], api: "/v1" },
    BackendSpec { name: "KoboldCpp", ports: &[5001], api: "/v1" },
    BackendSpec { name: "SGLang", ports: &[30000], api: "/v1" },
    BackendSpec { name: "llama-swap", ports: &[9297], api: "/v1" },
    BackendSpec { name: "Aphrodite", ports: &[2242], api: "/v1" },
];

/// Every standard port, plus a few common self-hosted ones. Kept sorted and deduplicated.
pub const PORTS: &[u16] = &[
    1234, 1337, 2242, 3000, 4891, 5000, 5001, 8000, 8080, 8081, 8888, 9000, 9297, 11434, 30000,
];

/// Context window used when a server does not report one. Local models are often small, so a
/// conservative value is safer than a huge one the runtime cannot honour.
pub const DEFAULT_CONTEXT: usize = 8_192;

const LOOPBACK_TIMEOUT: Duration = Duration::from_millis(320);
const LAN_TIMEOUT: Duration = Duration::from_millis(120);
const HTTP_TIMEOUT: Duration = Duration::from_millis(900);
const WORKERS: usize = 64;

/// One model offered by a discovered server. `base_url` already includes the API prefix, so the
/// agent can use it directly as a provider endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalModel {
    pub id: String,
    pub name: String,
    pub backend: String,
    pub host: String,
    pub port: u16,
    pub base_url: String,
    pub context: usize,
    /// `true` for servers on this machine, `false` for servers on the LAN.
    pub local: bool,
}

impl LocalModel {
    /// Stable identity for the composer's quick model picker (ids can repeat across servers).
    pub fn key(&self) -> String {
        format!("{}|{}", self.base_url, self.id)
    }

    pub fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// Result of one discovery pass.
#[derive(Clone, Debug, Default)]
pub struct ScanReport {
    pub models: Vec<LocalModel>,
    pub servers: usize,
    pub hosts: usize,
    pub ports: usize,
    pub note: String,
}

impl ScanReport {
    pub fn summary(&self) -> String {
        if self.note.is_empty() {
            format!("{} models from {} servers", self.models.len(), self.servers)
        } else {
            self.note.clone()
        }
    }
}

/// Which runtime owns a port, for the label shown next to a result.
pub fn backend_for_port(port: u16) -> Option<&'static BackendSpec> {
    BACKENDS.iter().find(|b| b.ports.contains(&port))
}

/// Best-effort address of this machine on the LAN, from the route the OS would use to reach the
/// internet. No packet is actually sent.
pub fn lan_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

/// Resolve an optional user-supplied host (IP or name) to an IPv4 address.
fn extra_ipv4(host: &str) -> Option<Ipv4Addr> {
    let host = host.trim();
    if host.is_empty() {
        return None;
    }
    if let Ok(IpAddr::V4(ip)) = host.parse::<IpAddr>() {
        return Some(ip);
    }
    (host, 0).to_socket_addrs().ok()?.find_map(|a| match a.ip() {
        IpAddr::V4(ip) => Some(ip),
        IpAddr::V6(_) => None,
    })
}

/// Expand the hosts to scan. Loopback is always included; the subnet is the /24 of the active
/// LAN address, which is what "same Wi-Fi" means in practice.
fn scan_hosts(include_lan: bool, extra_host: &str) -> (Vec<IpAddr>, String) {
    let mut hosts = vec![IpAddr::V4(Ipv4Addr::LOCALHOST)];
    let mut note = "Scanned localhost".to_string();

    if include_lan {
        match lan_ipv4() {
            Some(ip) => {
                let octets = ip.octets();
                for last in 1u8..=254 {
                    hosts.push(IpAddr::V4(Ipv4Addr::new(octets[0], octets[1], octets[2], last)));
                }
                note.push_str(&format!(" and {}.{}.{}.0/24", octets[0], octets[1], octets[2]));
            }
            None => note.push_str(" (no LAN route found — subnet scan skipped)"),
        }
    }
    if let Some(ip) = extra_ipv4(extra_host) {
        if !hosts.contains(&IpAddr::V4(ip)) {
            hosts.push(IpAddr::V4(ip));
            note.push_str(&format!(" and {ip}"));
        }
    }
    (hosts, note)
}

/// Probe a set of addresses with a bounded number of threads. Returns only addresses where
/// something accepted a TCP connection.
fn open_ports(targets: &[SocketAddr], timeout: Duration) -> Vec<SocketAddr> {
    let next = Arc::new(AtomicUsize::new(0));
    let found = Arc::new(Mutex::new(Vec::new()));
    let targets = Arc::new(targets.to_vec());
    let workers = WORKERS.min(targets.len().max(1));
    let mut handles = Vec::with_capacity(workers);
    for _ in 0..workers {
        let next = Arc::clone(&next);
        let found = Arc::clone(&found);
        let targets = Arc::clone(&targets);
        handles.push(thread::spawn(move || loop {
            let i = next.fetch_add(1, Ordering::Relaxed);
            if i >= targets.len() {
                break;
            }
            if TcpStream::connect_timeout(&targets[i], timeout).is_ok() {
                found.lock().map(|mut v| v.push(targets[i])).ok();
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }
    let mut out = found.lock().map(|v| v.clone()).unwrap_or_default();
    out.sort();
    out.dedup();
    out
}

fn json_string(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| v.get(*k).and_then(Value::as_str).map(str::to_string))
}

fn json_number(v: &Value, keys: &[&str]) -> Option<usize> {
    keys.iter().find_map(|k| v.get(*k).and_then(Value::as_u64).map(|n| n as usize))
}

/// Parse the two shapes used by local servers:
/// OpenAI (`{"data":[{"id":...}]}`) and Ollama's native tags (`{"models":[{"name":...}]}`).
fn parse_models(body: &str) -> Vec<(String, String, usize)> {
    let Ok(value) = serde_json::from_str::<Value>(body) else { return Vec::new() };
    let entries: &Vec<Value> = match value.get("data").and_then(Value::as_array) {
        Some(data) => data,
        None => match value.get("models").and_then(Value::as_array) {
            Some(models) => models,
            None => return Vec::new(),
        },
    };
    let mut out = Vec::new();
    for entry in entries {
        let id = json_string(entry, &["id", "name", "model"])
            .or_else(|| json_string(entry, &["model_id"]));
        let Some(id) = id else { continue };
        let name = json_string(entry, &["name", "id", "model"]).unwrap_or_else(|| id.clone());
        let context = json_number(entry, &["context_length", "context", "n_ctx", "max_model_len"])
            .or_else(|| entry.get("meta").and_then(|m| json_number(m, &["n_ctx", "context_length"])))
            .unwrap_or(DEFAULT_CONTEXT);
        out.push((id, name, context));
    }
    out
}

/// Ask one open address for its model list. Tries the runtime's own API prefix first, then the
/// unprefixed `/models` and Ollama's `/api/tags`, so slightly different builds are still found.
fn query_models(agent: &ureq::Agent, addr: SocketAddr, backend: Option<&BackendSpec>) -> Option<Vec<LocalModel>> {
    let root = format!("http://{addr}");
    let prefix = backend.map(|b| b.api).unwrap_or("/v1");
    let mut urls = vec![format!("{root}{prefix}/models"), format!("{root}/models")];
    if backend.map(|b| b.name) == Some("Ollama") {
        urls.push(format!("{root}/api/tags"));
    }
    for url in urls {
        let Ok(response) = agent.get(&url).call() else { continue };
        if response.status() != 200 {
            continue;
        }
        let Ok(body) = response.into_string() else { continue };
        let parsed = parse_models(&body);
        if parsed.is_empty() {
            continue;
        }
        let backend_name = backend.map(|b| b.name).unwrap_or("OpenAI-compatible").to_string();
        let host = addr.ip().to_string();
        let is_local = addr.ip().is_loopback();
        let base_url = format!("{root}{prefix}");
        return Some(parsed.into_iter().map(|(id, name, context)| LocalModel {
            id,
            name,
            backend: backend_name.clone(),
            host: host.clone(),
            port: addr.port(),
            base_url: base_url.clone(),
            context,
            local: is_local,
        }).collect());
    }
    None
}

fn scan_group(targets: &[SocketAddr], connect_timeout: Duration) -> ScanReport {
    let agent = ureq::AgentBuilder::new()
        .timeout(HTTP_TIMEOUT)
        .timeout_connect(connect_timeout)
        .build();
    let mut report = ScanReport {
        ports: targets.len(),
        ..ScanReport::default()
    };
    let mut seen_urls = std::collections::HashSet::new();
    let mut servers = 0usize;
    for addr in open_ports(targets, connect_timeout) {
        let backend = backend_for_port(addr.port());
        if let Some(models) = query_models(&agent, addr, backend) {
            if seen_urls.insert(format!("http://{addr}")) {
                servers += 1;
            }
            report.models.extend(models);
        }
    }
    report.models.sort_by(|a, b| {
        a.backend.cmp(&b.backend)
            .then(a.host.cmp(&b.host))
            .then(a.port.cmp(&b.port))
            .then(a.name.cmp(&b.name))
    });
    report.models.dedup_by(|a, b| a.base_url == b.base_url && a.id == b.id);
    report.servers = servers;
    report.note = format!("{} models from {} servers · {} ports probed", report.models.len(), servers, targets.len());
    report
}

/// Probe explicit addresses. Exposed so a custom port/host can be scanned (and unit tested)
/// without walking a whole subnet.
pub fn scan_targets(targets: &[SocketAddr]) -> ScanReport {
    scan_group(targets, LAN_TIMEOUT)
}

/// Full discovery pass: localhost (all standard ports), optionally the same /24 and one extra
/// host. Run this on a worker thread — a subnet scan can take several seconds.
pub fn scan(include_lan: bool, extra_host: &str) -> ScanReport {
    let (hosts, note) = scan_hosts(include_lan, extra_host);
    let mut local_targets = Vec::new();
    let mut lan_targets = Vec::new();
    for host in &hosts {
        let target = if host.is_loopback() { &mut local_targets } else { &mut lan_targets };
        for port in PORTS {
            target.push(SocketAddr::new(*host, *port));
        }
    }
    // Loopback refuses immediately, so it can afford a longer connect timeout; the subnet is
    // scanned with a short one to keep a 254-host sweep fast.
    let mut report = scan_group(&local_targets, LOOPBACK_TIMEOUT);
    let lan = scan_group(&lan_targets, LAN_TIMEOUT);
    report.models.extend(lan.models);
    report.servers += lan.servers;
    report.hosts = hosts.len();
    report.ports = local_targets.len() + lan_targets.len();
    report.models.sort_by(|a, b| {
        a.backend.cmp(&b.backend)
            .then(a.host.cmp(&b.host))
            .then(a.port.cmp(&b.port))
            .then(a.name.cmp(&b.name))
    });
    report.models.dedup_by(|a, b| a.base_url == b.base_url && a.id == b.id);
    report.note = format!("{note} · {} models from {} servers", report.models.len(), report.servers);
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{ErrorKind, Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn backends_cover_the_common_local_ports() {
        assert_eq!(backend_for_port(11434).map(|b| b.name), Some("Ollama"));
        assert_eq!(backend_for_port(1234).map(|b| b.name), Some("LM Studio"));
        assert_eq!(backend_for_port(8080).map(|b| b.name), Some("llama.cpp"));
        assert_eq!(backend_for_port(8000).map(|b| b.name), Some("vLLM"));
        assert!(backend_for_port(65500).is_none());
        let mut ports = PORTS.to_vec();
        ports.sort_unstable();
        ports.dedup();
        assert_eq!(ports.len(), PORTS.len());
    }

    #[test]
    fn parses_openai_and_ollama_model_lists() {
        let openai = r#"{"object":"list","data":[{"id":"qwen2.5:7b","name":"Qwen 2.5 7B","context_length":32768}]}"#;
        let parsed = parse_models(openai);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0], ("qwen2.5:7b".into(), "Qwen 2.5 7B".into(), 32_768));

        let tags = r#"{"models":[{"name":"llama3.2:latest","model":"llama3.2:latest"},{"name":"phi4:latest"}]}"#;
        let parsed = parse_models(tags);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0, "llama3.2:latest");
        assert_eq!(parsed[1].2, DEFAULT_CONTEXT);
        assert!(parse_models("not json").is_empty());
    }

    #[test]
    fn model_key_is_unique_per_endpoint() {
        let a = LocalModel {
            id: "m".into(), name: "M".into(), backend: "Ollama".into(), host: "127.0.0.1".into(),
            port: 11434, base_url: "http://127.0.0.1:11434/v1".into(), context: 8192, local: true,
        };
        let mut b = a.clone();
        b.host = "192.168.1.20".into();
        b.base_url = "http://192.168.1.20:11434/v1".into();
        assert_ne!(a.key(), b.key());
    }

    /// A tiny in-process OpenAI-compatible server. The first connection it sees is the TCP port
    /// probe (no request), the next ones ask for a model list.
    fn mock_models_server(listener: TcpListener, id: &str) -> (u16, Arc<AtomicBool>, thread::JoinHandle<()>) {
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let id = id.to_string();
        let handle = thread::spawn(move || {
            let body = format!(
                r#"{{"data":[{{"id":"{id}","name":"Local Test","context_length":16384}}]}}"#
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(), body
            );
            while !stop_thread.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut buf = [0u8; 2048];
                        let _ = stream.read(&mut buf);
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(5)),
                    Err(_) => break,
                }
            }
        });
        (port, stop, handle)
    }

    /// Exercises the whole path: TCP probe -> HTTP request -> parse -> model entry.
    #[test]
    fn discovers_a_running_server() {
        let (port, stop, handle) = mock_models_server(TcpListener::bind("127.0.0.1:0").unwrap(), "local-test");
        let report = scan_targets(&[SocketAddr::from(([127, 0, 0, 1], port))]);
        stop.store(true, Ordering::Relaxed);
        handle.join().unwrap();
        assert_eq!(report.servers, 1);
        assert_eq!(report.models.len(), 1);
        assert_eq!(report.models[0].id, "local-test");
        assert_eq!(report.models[0].context, 16_384);
        assert!(report.models[0].local);
    }

    /// The full loopback sweep used by the UI must find a server on a standard port and label it
    /// with the matching runtime. Skipped when the port is already in use on the test machine.
    #[test]
    fn loopback_scan_finds_a_standard_port() {
        let Ok(listener) = TcpListener::bind("127.0.0.1:1234") else { return };
        let (_port, stop, handle) = mock_models_server(listener, "standard-port-test");
        let report = scan(false, "");
        stop.store(true, Ordering::Relaxed);
        handle.join().unwrap();
        let found = report.models.iter().find(|m| m.id == "standard-port-test");
        assert!(found.is_some(), "loopback scan did not find the mock server");
        let found = found.unwrap();
        assert_eq!(found.backend, "LM Studio");
        assert_eq!(found.base_url, "http://127.0.0.1:1234/v1");
        assert!(found.local);
    }
}
