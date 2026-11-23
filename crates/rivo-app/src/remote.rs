//! Local LAN bridge for the future Rivo phone app.
//!
//! A tiny HTTP server runs on the desktop and exposes sessions, turns, info and checkpoints
//! as JSON. The phone pairs once with the code shown in Settings, then sends that code as a
//! bearer token. The bridge listens on all interfaces but is meant for a trusted LAN: the
//! pairing code is the only credential, so regenerate it if a device is lost.

use std::io::{Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Value, json};

/// A request from the bridge that must be handled on the UI thread.
pub enum RemoteCommand {
    Status { reply: mpsc::Sender<Result<Value, String>> },
    Pair { name: String, ip: String, reply: mpsc::Sender<Result<Value, String>> },
    ListSessions { reply: mpsc::Sender<Result<Value, String>> },
    GetSession { id: String, reply: mpsc::Sender<Result<Value, String>> },
    NewSession { title: String, reply: mpsc::Sender<Result<Value, String>> },
    Send { id: String, text: String, reply: mpsc::Sender<Result<Value, String>> },
    Checkpoints { reply: mpsc::Sender<Result<Value, String>> },
    SaveCheckpoint { label: String, reply: mpsc::Sender<Result<Value, String>> },
    RestoreCheckpoint { id: String, reply: mpsc::Sender<Result<Value, String>> },
}

/// A device that paired with this desktop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteDevice {
    pub name: String,
    pub ip: String,
    pub since: String,
}

/// Handle for the background listener. Dropping it or calling `stop` shuts the server down.
pub struct RemoteServer {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl RemoteServer {
    pub fn start(port: u16, token: String, tx: mpsc::Sender<RemoteCommand>) -> Result<Self, String> {
        let listener = TcpListener::bind(("0.0.0.0", port)).map_err(|e| format!("port {port}: {e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = thread::spawn(move || serve(listener, token, tx, flag));
        Ok(Self { stop, thread: Some(thread) })
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for RemoteServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Best-effort LAN address, used only to show a clickable URL in Settings.
pub fn lan_address(port: u16) -> String {
    let ip = (|| {
        let s = UdpSocket::bind("0.0.0.0:0").ok()?;
        s.connect("8.8.8.8:80").ok()?;
        s.local_addr().ok().map(|a| a.ip())
    })().unwrap_or(IpAddr::from([127, 0, 0, 1]));
    format!("http://{ip}:{port}")
}

/// Cryptographically random pairing code. Falls back to a time-based value on exotic targets.
pub fn new_code() -> String {
    if let Ok(mut bytes) = std::fs::File::open("/dev/urandom") {
        let mut raw = [0u8; 12];
        if bytes.read_exact(&mut raw).is_ok() {
            return raw.iter().map(|b| format!("{b:02x}")).collect();
        }
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{nanos:024x}")[..24].to_string()
}

fn serve(listener: TcpListener, token: String, tx: mpsc::Sender<RemoteCommand>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, addr)) => {
                let token = token.clone();
                let tx = tx.clone();
                let _ = handle_stream(stream, addr.ip().to_string(), token, tx);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(40));
            }
            Err(_) => break,
        }
    }
}

fn handle_stream(mut stream: TcpStream, ip: String, token: String, tx: mpsc::Sender<RemoteCommand>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let (method, path, headers, body) = loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 { return Ok(()); }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..pos]).to_string();
            let mut lines = head.lines();
            let request = lines.next().unwrap_or("").split_whitespace().collect::<Vec<_>>();
            if request.len() < 2 { return Ok(()); }
            let method = request[0].to_string();
            let path = request[1].to_string();
            let mut headers = std::collections::HashMap::new();
            for line in lines {
                if let Some((k, v)) = line.split_once(':') {
                    headers.insert(k.trim().to_lowercase(), v.trim().to_string());
                }
            }
            let len = headers.get("content-length").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
            let body_start = pos + 4;
            if buf.len() >= body_start + len {
                let body = buf[body_start..body_start + len].to_vec();
                break (method, path, headers, body);
            }
        }
        if buf.len() > 1024 * 1024 { return Ok(()); }
    };

    let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
    let route = path.split('?').next().unwrap_or(&path).to_string();
    let args: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let query_token = query_value(query, "token");
    let authorized = headers.get("authorization")
        .and_then(|v| v.strip_prefix("Bearer "))
        .or(query_token.as_deref())
        .is_some_and(|given| given == token);

    let result = if route == "/api/pair" && method == "POST" {
        let code = args.get("code").and_then(Value::as_str).unwrap_or("");
        if code == token {
            let name = args.get("name").and_then(Value::as_str).unwrap_or("phone").to_string();
            ask(&tx, |reply| RemoteCommand::Pair { name, ip, reply })
        } else {
            Err("invalid pairing code".into())
        }
    } else if !authorized {
        Err("unauthorized: pair first or send the bearer token".into())
    } else if route == "/api/status" && method == "GET" {
        ask(&tx, |reply| RemoteCommand::Status { reply })
    } else if route == "/api/sessions" && method == "GET" {
        ask(&tx, |reply| RemoteCommand::ListSessions { reply })
    } else if route == "/api/sessions" && method == "POST" {
        let title = args.get("title").and_then(Value::as_str).unwrap_or("New session").to_string();
        ask(&tx, |reply| RemoteCommand::NewSession { title, reply })
    } else if route == "/api/checkpoints" && method == "GET" {
        ask(&tx, |reply| RemoteCommand::Checkpoints { reply })
    } else if route == "/api/checkpoints" && method == "POST" {
        let label = args.get("label").and_then(Value::as_str).unwrap_or("").to_string();
        ask(&tx, |reply| RemoteCommand::SaveCheckpoint { label, reply })
    } else if route == "/api/checkpoints/restore" && method == "POST" {
        let id = args.get("id").and_then(Value::as_str).unwrap_or("").to_string();
        ask(&tx, |reply| RemoteCommand::RestoreCheckpoint { id, reply })
    } else if let Some(id) = route.strip_prefix("/api/sessions/") {
        let id = id.trim_end_matches("/message").trim_end_matches('/').to_string();
        if route.ends_with("/message") && method == "POST" {
            let text = args.get("text").and_then(Value::as_str).unwrap_or("").to_string();
            ask(&tx, |reply| RemoteCommand::Send { id, text, reply })
        } else if method == "GET" {
            ask(&tx, |reply| RemoteCommand::GetSession { id, reply })
        } else {
            Err("not found".into())
        }
    } else {
        Err("not found".into())
    };

    let (status, payload) = match result {
        Ok(v) => (200, v),
        Err(e) => (401, json!({ "error": e })),
    };
    let body = serde_json::to_vec(&payload).unwrap_or_else(|_| b"{}".to_vec());
    let head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        if status == 200 { "OK" } else { "Unauthorized" },
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&body)?;
    stream.flush()
}

fn query_value(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| percent_decode(v))
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn ask(tx: &mpsc::Sender<RemoteCommand>, build: impl FnOnce(mpsc::Sender<Result<Value, String>>) -> RemoteCommand) -> Result<Value, String> {
    let (rtx, rx) = mpsc::channel();
    tx.send(build(rtx)).map_err(|e| format!("bridge is stopped: {e}"))?;
    rx.recv_timeout(Duration::from_secs(10)).unwrap_or_else(|e| Err(format!("desktop did not answer: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_are_decoded() {
        assert_eq!(query_value("a=1&token=abc%20def", "token").as_deref(), Some("abc def"));
        assert_eq!(query_value("a=1", "b"), None);
    }

    #[test]
    fn codes_are_hex_and_distinct() {
        let a = new_code();
        let b = new_code();
        assert_eq!(a.len(), 24);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }
}
