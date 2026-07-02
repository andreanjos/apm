use super::*;
use std::{
    fs,
    io::{Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    path::{Path, PathBuf},
};

use apm_core::service::LOOPBACK_TOKEN_HEADER;

use crate::service::DesktopServiceStatus;

pub(super) fn write_test_token(name: &str) -> PathBuf {
    let token_path = std::env::temp_dir().join(format!(
        "apm-desktop-{name}-token-{}.json",
        std::process::id()
    ));
    fs::write(&token_path, "{\"token\":\"secret\"}").expect("write token file");
    token_path
}

pub(super) fn test_session(port: u16, token_path: &Path) -> DesktopServiceSession {
    let contract = apm_core::service::local_service_contract();
    DesktopServiceSession {
        status: DesktopServiceStatus::Started,
        url: format!("http://127.0.0.1:{port}"),
        pid: None,
        api_version: contract.api_version,
        schema_version: contract.schema_version,
        token_header: LOOPBACK_TOKEN_HEADER.to_string(),
        token_file: token_path.display().to_string(),
        token_available: true,
        privileged_install_policy: contract.security.privileged_install_policy,
        pending_runtime_work: contract.pending_runtime_work,
        message: "test".to_string(),
    }
}

pub(super) struct Request {
    pub(super) stream: TcpStream,
    text: String,
}

impl Request {
    pub(super) fn contains(&self, needle: &str) -> bool {
        self.text.contains(needle)
    }
}

pub(super) fn accept_request(listener: &TcpListener) -> Request {
    let (mut stream, _) = listener.accept().expect("accept service request");
    let text = read_request(&mut stream);
    Request { stream, text }
}

fn read_request(stream: &mut TcpStream) -> String {
    let mut request_bytes = Vec::new();
    let mut expected_len = None;
    loop {
        let mut buffer = [0_u8; 512];
        let read = stream.read(&mut buffer).expect("read request");
        if read == 0 {
            break;
        }
        request_bytes.extend_from_slice(&buffer[..read]);
        if expected_len.is_none() {
            expected_len = expected_request_len(&request_bytes);
        }
        if expected_len.is_some_and(|len| request_bytes.len() >= len) {
            break;
        }
    }
    String::from_utf8_lossy(&request_bytes).into_owned()
}

fn expected_request_len(request_bytes: &[u8]) -> Option<usize> {
    let header_end = request_bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")?;
    let headers = String::from_utf8_lossy(&request_bytes[..header_end]);
    Some(header_end + 4 + content_length(&headers))
}

fn content_length(headers: &str) -> usize {
    headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())
                .flatten()
        })
        .unwrap_or(0)
}

pub(super) fn write_json_response(mut stream: TcpStream, status: &str, body: &str) {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .expect("write response");
    stream.flush().expect("flush response");
    let _ = stream.shutdown(Shutdown::Write);
}

pub(super) fn write_event_stream_response(mut stream: TcpStream, body: &str) {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .expect("write response");
    stream.flush().expect("flush response");
    let _ = stream.shutdown(Shutdown::Write);
}
