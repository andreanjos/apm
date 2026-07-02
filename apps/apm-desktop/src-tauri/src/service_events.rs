use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    time::Duration,
};

use apm_core::{engine::EngineEvent, service::LOOPBACK_TOKEN_HEADER};

const CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
const STREAM_READ_TIMEOUT: Duration = Duration::from_secs(60);

pub fn stream_operation_events(
    port: u16,
    token: &str,
    operation_id: &str,
    status_url: &str,
    on_event: &mut impl FnMut(&str, EngineEvent),
) -> Result<(), String> {
    let events_path = format!("{status_url}/events");
    let stream = open_event_stream(port, token, &events_path)?;
    let mut reader = BufReader::new(stream);
    let headers = read_response_headers(&mut reader)?;
    if status_code(&headers) != Some(200) {
        let mut body = String::new();
        let _ = reader.read_to_string(&mut body);
        return Err(service_error_message(&headers, &body));
    }

    let mut emit = |event| on_event(operation_id, event);
    if is_chunked_response(&headers) {
        read_chunked_sse_events(&mut reader, &mut emit)
    } else {
        read_plain_sse_events(&mut reader, &mut emit)
    }
}

fn open_event_stream(port: u16, token: &str, path: &str) -> Result<TcpStream, String> {
    let mut stream = TcpStream::connect_timeout(&service_addr(port), CONNECT_TIMEOUT)
        .map_err(|error| format!("service is not reachable: {error}"))?;
    stream
        .set_read_timeout(Some(STREAM_READ_TIMEOUT))
        .map_err(|error| format!("failed to set service event timeout: {error}"))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n{LOOPBACK_TOKEN_HEADER}: {token}\r\nContent-Length: 0\r\n\r\n",
    )
    .map_err(|error| format!("failed to write service event request: {error}"))?;
    Ok(stream)
}

fn read_response_headers(reader: &mut impl BufRead) -> Result<String, String> {
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .map_err(|error| format!("failed to read service event headers: {error}"))?;
        if read == 0 {
            return Err("service event response ended before headers completed".to_string());
        }
        headers.push_str(&line);
        if line == "\r\n" || line == "\n" {
            return Ok(headers);
        }
    }
}

fn read_plain_sse_events(
    reader: &mut impl Read,
    on_event: &mut impl FnMut(EngineEvent),
) -> Result<(), String> {
    let mut parser = SseEventParser::default();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("failed to read service event stream: {error}"))?;
        if read == 0 {
            return parser.finish(on_event);
        }
        parser.push_bytes(&buffer[..read], on_event)?;
    }
}

fn read_chunked_sse_events(
    reader: &mut impl BufRead,
    on_event: &mut impl FnMut(EngineEvent),
) -> Result<(), String> {
    let mut parser = SseEventParser::default();
    loop {
        let mut size_line = String::new();
        let read = reader
            .read_line(&mut size_line)
            .map_err(|error| format!("failed to read service event chunk size: {error}"))?;
        if read == 0 {
            return parser.finish(on_event);
        }

        let size_line = size_line.trim();
        if size_line.is_empty() {
            continue;
        }
        let size_hex = size_line.split(';').next().unwrap_or(size_line);
        let size = usize::from_str_radix(size_hex, 16)
            .map_err(|error| format!("service event stream had invalid chunk size: {error}"))?;
        if size == 0 {
            return parser.finish(on_event);
        }

        let mut chunk = vec![0_u8; size];
        reader
            .read_exact(&mut chunk)
            .map_err(|error| format!("failed to read service event chunk: {error}"))?;
        parser.push_bytes(&chunk, on_event)?;

        let mut crlf = [0_u8; 2];
        reader
            .read_exact(&mut crlf)
            .map_err(|error| format!("failed to read service event chunk terminator: {error}"))?;
        if crlf != *b"\r\n" {
            return Err("service event stream had an invalid chunk terminator".to_string());
        }
    }
}

#[derive(Default)]
struct SseEventParser {
    pending: Vec<u8>,
    event_name: Option<String>,
    data_lines: Vec<String>,
}

impl SseEventParser {
    fn push_bytes(
        &mut self,
        bytes: &[u8],
        on_event: &mut impl FnMut(EngineEvent),
    ) -> Result<(), String> {
        self.pending.extend_from_slice(bytes);
        while let Some(newline) = self.pending.iter().position(|byte| *byte == b'\n') {
            let mut line_bytes: Vec<u8> = self.pending.drain(..=newline).collect();
            trim_line_end(&mut line_bytes);
            let line = String::from_utf8(line_bytes)
                .map_err(|error| format!("service event stream was not UTF-8: {error}"))?;
            self.push_line(&line, on_event)?;
        }
        Ok(())
    }

    fn finish(&mut self, on_event: &mut impl FnMut(EngineEvent)) -> Result<(), String> {
        if !self.pending.is_empty() {
            let line = String::from_utf8(std::mem::take(&mut self.pending))
                .map_err(|error| format!("service event stream was not UTF-8: {error}"))?;
            self.push_line(&line, on_event)?;
        }
        self.flush(on_event)
    }

    fn push_line(
        &mut self,
        line: &str,
        on_event: &mut impl FnMut(EngineEvent),
    ) -> Result<(), String> {
        if line.is_empty() {
            return self.flush(on_event);
        }
        if line.starts_with(':') {
            return Ok(());
        }

        let (field, value) = line
            .split_once(':')
            .map(|(field, value)| (field, value.strip_prefix(' ').unwrap_or(value)))
            .unwrap_or((line, ""));
        match field {
            "event" => self.event_name = Some(value.to_string()),
            "data" => self.data_lines.push(value.to_string()),
            _ => {}
        }
        Ok(())
    }

    fn flush(&mut self, on_event: &mut impl FnMut(EngineEvent)) -> Result<(), String> {
        if self.event_name.as_deref() == Some("engine_event") && !self.data_lines.is_empty() {
            let data = self.data_lines.join("\n");
            let event = serde_json::from_str::<EngineEvent>(&data)
                .map_err(|error| format!("service event stream had invalid event JSON: {error}"))?;
            on_event(event);
        }
        self.event_name = None;
        self.data_lines.clear();
        Ok(())
    }
}

fn trim_line_end(line: &mut Vec<u8>) {
    if line.last() == Some(&b'\n') {
        line.pop();
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
}

fn status_code(head: &str) -> Option<u16> {
    head.lines().next()?.split_whitespace().nth(1)?.parse().ok()
}

fn is_chunked_response(headers: &str) -> bool {
    headers.lines().any(|line| {
        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        name.eq_ignore_ascii_case("transfer-encoding")
            && value
                .split(',')
                .any(|encoding| encoding.trim().eq_ignore_ascii_case("chunked"))
    })
}

fn service_error_message(head: &str, body: &str) -> String {
    let status = head.lines().next().unwrap_or("unknown status");
    format!("service returned {status}: {}", body.trim())
}

fn service_addr(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn plain_sse_parser_emits_engine_events() {
        let body = b"event: engine_event\ndata: {\"event\":\"install_state_recorded\",\"slug\":\"surge-xt\"}\n\n";
        let mut events = Vec::new();

        read_plain_sse_events(&mut Cursor::new(body), &mut |event| events.push(event))
            .expect("plain SSE should parse");

        assert_eq!(
            events,
            vec![EngineEvent::InstallStateRecorded {
                slug: "surge-xt".to_string()
            }]
        );
    }

    #[test]
    fn chunked_sse_parser_emits_engine_events() {
        let chunks = [
            "event: engine_event\n",
            "data: {\"event\":\"remove_state_recorded\",\"slug\":\"surge-xt\"}\n\n",
        ];
        let mut chunked = Vec::new();
        for chunk in chunks {
            chunked.extend_from_slice(format!("{:x}\r\n", chunk.len()).as_bytes());
            chunked.extend_from_slice(chunk.as_bytes());
            chunked.extend_from_slice(b"\r\n");
        }
        chunked.extend_from_slice(b"0\r\n\r\n");

        let mut events = Vec::new();
        read_chunked_sse_events(&mut BufReader::new(Cursor::new(chunked)), &mut |event| {
            events.push(event);
        })
        .expect("chunked SSE should parse");

        assert_eq!(
            events,
            vec![EngineEvent::RemoveStateRecorded {
                slug: "surge-xt".to_string()
            }]
        );
    }
}
