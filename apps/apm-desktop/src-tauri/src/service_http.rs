use std::{
    fs,
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    path::Path,
    time::Duration,
};

use apm_core::service::LOOPBACK_TOKEN_HEADER;
use serde::{de::DeserializeOwned, Deserialize};

const CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
const READ_TIMEOUT: Duration = Duration::from_secs(5);

pub struct ServiceHttpClient {
    port: u16,
    token: String,
}

#[derive(Debug, Deserialize)]
struct LoopbackTokenFile {
    token: String,
}

impl ServiceHttpClient {
    pub fn new(port: u16, token: String) -> Self {
        Self { port, token }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        let response = self.request("GET", path, "")?;
        parse_json_response(&response, 200)
    }

    pub fn post_json_ok<T: DeserializeOwned>(&self, path: &str, body: &str) -> Result<T, String> {
        self.post_json(path, body, 200)
    }

    pub fn post_json_accepted<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &str,
    ) -> Result<T, String> {
        self.post_json(path, body, 202)
    }

    pub fn delete_json_ok<T: DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        let response = self.request("DELETE", path, "")?;
        parse_json_response(&response, 200)
    }

    fn post_json<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &str,
        expected_status: u16,
    ) -> Result<T, String> {
        let response = self.request("POST", path, body)?;
        parse_json_response(&response, expected_status)
    }

    fn request(&self, method: &str, path: &str, body: &str) -> Result<String, String> {
        let mut stream = TcpStream::connect_timeout(&service_addr(self.port), CONNECT_TIMEOUT)
            .map_err(|error| format!("service is not reachable: {error}"))?;
        stream
            .set_read_timeout(Some(READ_TIMEOUT))
            .map_err(|error| format!("failed to set service read timeout: {error}"))?;
        let content_type = if body.is_empty() {
            String::new()
        } else {
            "Content-Type: application/json\r\n".to_string()
        };
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n{content_type}{LOOPBACK_TOKEN_HEADER}: {}\r\nContent-Length: {}\r\n\r\n{body}",
            self.token,
            body.len()
        )
        .map_err(|error| format!("failed to write service request: {error}"))?;

        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .map_err(|error| format!("failed to read service response: {error}"))?;
        Ok(response)
    }
}

pub fn token_available(path: &Path) -> bool {
    read_loopback_token(path).is_ok()
}

pub fn read_loopback_token(path: &Path) -> Result<String, String> {
    let content = fs::read_to_string(path)
        .map_err(|error| format!("failed to read loopback token: {error}"))?;
    let token_file: LoopbackTokenFile = serde_json::from_str(&content)
        .map_err(|error| format!("failed to parse loopback token: {error}"))?;
    let token = token_file.token.trim();
    if token.is_empty() {
        return Err("loopback token file is empty".to_string());
    }
    Ok(token.to_string())
}

pub fn service_port(url: &str) -> Result<u16, String> {
    url.strip_prefix("http://127.0.0.1:")
        .ok_or_else(|| format!("unsupported service URL: {url}"))?
        .parse::<u16>()
        .map_err(|error| format!("service URL has invalid port: {error}"))
}

fn parse_json_response<T: DeserializeOwned>(
    response: &str,
    expected_status: u16,
) -> Result<T, String> {
    let (head, body) = split_http_response(response)?;
    if status_code(head) != Some(expected_status) {
        return Err(service_error_message(head, body));
    }
    serde_json::from_str(body.trim())
        .map_err(|error| format!("service response was not valid JSON: {error}"))
}

fn status_code(head: &str) -> Option<u16> {
    head.lines().next()?.split_whitespace().nth(1)?.parse().ok()
}

fn split_http_response(response: &str) -> Result<(&str, &str), String> {
    response
        .split_once("\r\n\r\n")
        .ok_or_else(|| "service response was not HTTP".to_string())
}

fn service_error_message(head: &str, body: &str) -> String {
    #[derive(Deserialize)]
    struct ErrorBody {
        error: String,
    }

    let status = head.lines().next().unwrap_or("unknown status");
    match serde_json::from_str::<ErrorBody>(body.trim()) {
        Ok(error) => format!("service returned {status}: {}", error.error),
        Err(_) => format!("service returned {status}"),
    }
}

fn service_addr(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_port_accepts_loopback_url() {
        assert_eq!(service_port("http://127.0.0.1:4767").unwrap(), 4767);
    }

    #[test]
    fn service_port_rejects_non_loopback_url() {
        let error = service_port("http://localhost:4767").expect_err("URL should be rejected");

        assert!(error.contains("unsupported service URL"));
    }

    #[test]
    fn parse_json_response_returns_service_error_body() {
        let error = parse_json_response::<serde_json::Value>(
            "HTTP/1.1 401 Unauthorized\r\n\r\n{\"error\":\"Missing token\"}",
            200,
        )
        .expect_err("HTTP error should be rejected");

        assert!(error.contains("401 Unauthorized"));
        assert!(error.contains("Missing token"));
    }
}
