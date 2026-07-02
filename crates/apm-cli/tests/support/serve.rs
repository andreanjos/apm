use std::{
    fs,
    io::{ErrorKind, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Stdio},
    sync::{Mutex, MutexGuard},
    thread,
    time::{Duration, Instant},
};

use apm_core::service::LOOPBACK_TOKEN_HEADER;

use super::{command, CliTestEnv};

static SERVER_TEST_LOCK: Mutex<()> = Mutex::new(());

pub struct ChildGuard {
    child: Child,
    _lock: MutexGuard<'static, ()>,
}

pub struct TestServer {
    pub url: String,
    handle: thread::JoinHandle<()>,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl TestServer {
    pub fn join(self) {
        self.handle.join().expect("server thread should finish");
    }
}

pub fn spawn_server(env: &CliTestEnv, port: u16) -> ChildGuard {
    let lock = SERVER_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let child = command(env)
        .args(["serve", "run", "--port", &port.to_string(), "--quiet"])
        .env("HOME", env.data_home.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn apm serve run");

    ChildGuard { child, _lock: lock }
}

pub fn free_loopback_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind free loopback port")
        .local_addr()
        .expect("read free loopback addr")
        .port()
}

pub fn wait_for_operation_state(
    env: &CliTestEnv,
    port: u16,
    status_url: &str,
    expected_state: &str,
) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let response = http_request_with_auth(env, "GET", port, status_url, "");
        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "operation status should return 200, got: {response}"
        );
        let status: serde_json::Value =
            serde_json::from_str(response_body(&response)).expect("operation status JSON");
        if status["state"] == expected_state {
            return status;
        }
        if status["state"] == "failed" {
            panic!("operation failed: {status}");
        }
        if Instant::now() >= deadline {
            panic!("timed out waiting for operation state {expected_state}: {status}");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

pub fn wait_for_persisted_operation(env: &CliTestEnv, operation_id: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let history_path = operation_history_path(env);
    loop {
        if let Ok(content) = fs::read_to_string(&history_path) {
            let history: serde_json::Value =
                serde_json::from_str(&content).expect("operation history should be JSON");
            let persisted = history["operations"]
                .as_array()
                .expect("operation history should include operations")
                .iter()
                .any(|operation| operation["operation_id"] == operation_id);
            if persisted {
                return;
            }
        }
        if Instant::now() >= deadline {
            panic!(
                "timed out waiting for persisted operation {operation_id} at {}",
                history_path.display()
            );
        }
        thread::sleep(Duration::from_millis(50));
    }
}

pub fn operation_history_path(env: &CliTestEnv) -> PathBuf {
    env.data_home.path().join("apm/service/operations.json")
}

pub fn operation_token_path(env: &CliTestEnv) -> PathBuf {
    env.data_home.path().join("apm/service/token.json")
}

pub fn wait_for_http(method: &str, port: u16, path: &str, body: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match try_http_request_with_extra_headers(method, port, path, body, "") {
            Ok(response) => return response,
            Err(error) if Instant::now() < deadline => {
                let _ = error;
                thread::sleep(Duration::from_millis(50));
            }
            Err(error) => panic!("timed out waiting for apm service: {error}"),
        }
    }
}

pub fn wait_for_http_with_auth(
    env: &CliTestEnv,
    method: &str,
    port: u16,
    path: &str,
    body: &str,
) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match try_http_request_with_auth(env, method, port, path, body) {
            Ok(response) => return response,
            Err(error) if Instant::now() < deadline => {
                let _ = error;
                thread::sleep(Duration::from_millis(50));
            }
            Err(error) => panic!("timed out waiting for authenticated apm service: {error}"),
        }
    }
}

pub fn http_request_with_auth(
    env: &CliTestEnv,
    method: &str,
    port: u16,
    path: &str,
    body: &str,
) -> String {
    try_http_request_with_auth(env, method, port, path, body).expect("send HTTP request")
}

pub fn http_request_with_extra_headers(
    method: &str,
    port: u16,
    path: &str,
    body: &str,
    extra_headers: &str,
) -> String {
    try_http_request_with_extra_headers(method, port, path, body, extra_headers)
        .expect("send HTTP request")
}

pub fn serve_once(body: Vec<u8>) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let url = format!(
        "http://{}/update-effect.zip",
        listener.local_addr().expect("local addr")
    );
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut buffer = [0_u8; 1024];
        let _ = stream.read(&mut buffer);
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(header.as_bytes()).expect("write header");
        stream.write_all(&body).expect("write body");
    });
    TestServer { url, handle }
}

pub fn response_body(response: &str) -> &str {
    response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("HTTP response should contain a body")
        .trim()
}

fn try_http_request_with_auth(
    env: &CliTestEnv,
    method: &str,
    port: u16,
    path: &str,
    body: &str,
) -> std::io::Result<String> {
    let token = read_loopback_token(env);
    try_http_request_with_extra_headers(
        method,
        port,
        path,
        body,
        &format!("{LOOPBACK_TOKEN_HEADER}: {token}\r\n"),
    )
}

fn try_http_request_with_extra_headers(
    method: &str,
    port: u16,
    path: &str,
    body: &str,
    extra_headers: &str,
) -> std::io::Result<String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(200))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let content_type = if body.is_empty() {
        String::new()
    } else {
        "Content-Type: application/json\r\n".to_string()
    };
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n{content_type}{extra_headers}Content-Length: {}\r\n\r\n{body}",
        body.len()
    )?;

    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

fn read_loopback_token(env: &CliTestEnv) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    let token_path = operation_token_path(env);
    loop {
        match fs::read_to_string(&token_path) {
            Ok(content) => match parse_loopback_token(&content) {
                Ok(token) => return token,
                Err(error) if Instant::now() < deadline => {
                    let _ = error;
                }
                Err(error) => panic!(
                    "loopback token file at {} was not ready JSON before timeout: {error}",
                    token_path.display()
                ),
            },
            Err(error) if error.kind() == ErrorKind::NotFound && Instant::now() < deadline => {}
            Err(error) if Instant::now() < deadline => {
                let _ = error;
            }
            Err(error) => panic!(
                "timed out waiting for loopback token at {}: {error}",
                token_path.display()
            ),
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn parse_loopback_token(content: &str) -> Result<String, String> {
    let token_file: serde_json::Value =
        serde_json::from_str(content).map_err(|error| error.to_string())?;
    token_file["token"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "loopback token file should include token".to_string())
}
