use std::{
    env, fmt,
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

use apm_core::{
    config,
    service::{
        local_service_contract, loopback_token_file, LocalServiceContract, PrivilegedInstallPolicy,
        ServiceHealth, LOOPBACK_TOKEN_HEADER,
    },
};
use serde::{de::DeserializeOwned, Serialize};

use crate::service_http::token_available;

const CLI_PATH_ENV: &str = "APM_DESKTOP_CLI";
const SERVICE_STARTUP_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
const READ_TIMEOUT: Duration = Duration::from_secs(2);

pub struct DesktopServiceSupervisor {
    process: Mutex<ServiceProcess>,
}

#[derive(Default)]
struct ServiceProcess {
    child: Option<Child>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DesktopServiceSession {
    pub status: DesktopServiceStatus,
    pub url: String,
    pub pid: Option<u32>,
    pub api_version: String,
    pub schema_version: String,
    pub token_header: String,
    pub token_file: String,
    pub token_available: bool,
    pub privileged_install_policy: PrivilegedInstallPolicy,
    pub pending_runtime_work: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopServiceStatus {
    NotStarted,
    Reused,
    Started,
    Unavailable,
}

impl Default for DesktopServiceSupervisor {
    fn default() -> Self {
        Self {
            process: Mutex::new(ServiceProcess::default()),
        }
    }
}

impl DesktopServiceSupervisor {
    pub fn status(&self) -> Result<DesktopServiceSession, String> {
        let mut process = self.lock_process()?;
        let exited = process.reap_exited_child()?;
        let pid = process.child.as_ref().map(Child::id);
        let bind = service_bind();

        match probe_ready_service(bind.port) {
            Ok(ready) if pid.is_some() => Ok(session_from_ready_service(
                DesktopServiceStatus::Started,
                pid,
                ready,
                "Local service launched by apm desktop".to_string(),
            )),
            Ok(ready) => Ok(session_from_ready_service(
                DesktopServiceStatus::Reused,
                None,
                ready,
                "Reusing local apm service".to_string(),
            )),
            Err(error) if pid.is_some() => Ok(unavailable_session(
                bind,
                pid,
                format!("Local service process is running but not ready: {error}"),
            )),
            Err(error) if error.is_unreachable() => Ok(not_started_session(bind, exited)),
            Err(error) => Ok(unavailable_session(
                bind,
                None,
                format!("Local service is reachable but not ready: {error}"),
            )),
        }
    }

    pub fn ensure_started(&self) -> Result<DesktopServiceSession, String> {
        let mut process = self.lock_process()?;
        process.reap_exited_child()?;
        let bind = service_bind();

        match probe_ready_service(bind.port) {
            Ok(ready) => {
                let pid = process.child.as_ref().map(Child::id);
                let status = if pid.is_some() {
                    DesktopServiceStatus::Started
                } else {
                    DesktopServiceStatus::Reused
                };
                let message = if pid.is_some() {
                    "Local service launched by apm desktop".to_string()
                } else {
                    "Reusing local apm service".to_string()
                };
                return Ok(session_from_ready_service(status, pid, ready, message));
            }
            Err(error) if !error.is_unreachable() => {
                return Err(format!("Local service is reachable but not ready: {error}"));
            }
            Err(_) => {}
        }

        if let Some(child) = process.child.as_mut() {
            let ready = wait_for_ready_service(bind.port, child)?;
            return Ok(session_from_ready_service(
                DesktopServiceStatus::Started,
                Some(child.id()),
                ready,
                "Local service launched by apm desktop".to_string(),
            ));
        }

        let (mut child, cli_path) = spawn_service_process(bind.port)?;
        match wait_for_ready_service(bind.port, &mut child) {
            Ok(ready) => {
                let pid = child.id();
                process.child = Some(child);
                Ok(session_from_ready_service(
                    DesktopServiceStatus::Started,
                    Some(pid),
                    ready,
                    format!("Started local service with {}", cli_path.display()),
                ))
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(error)
            }
        }
    }

    fn lock_process(&self) -> Result<std::sync::MutexGuard<'_, ServiceProcess>, String> {
        self.process
            .lock()
            .map_err(|_| "local service supervisor lock poisoned".to_string())
    }
}

impl ServiceProcess {
    fn reap_exited_child(&mut self) -> Result<Option<ExitStatus>, String> {
        let Some(child) = self.child.as_mut() else {
            return Ok(None);
        };

        match child
            .try_wait()
            .map_err(|error| format!("Failed to inspect local service process: {error}"))?
        {
            Some(status) => {
                self.child = None;
                Ok(Some(status))
            }
            None => Ok(None),
        }
    }
}

impl Drop for ServiceProcess {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn spawn_service_process(port: u16) -> Result<(Child, PathBuf), String> {
    let mut errors = Vec::new();
    for cli_path in cli_candidates() {
        let mut command = Command::new(&cli_path);
        command
            .args([
                "serve",
                "run",
                "--host",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                "--quiet",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        match command.spawn() {
            Ok(child) => return Ok((child, cli_path)),
            Err(error) => errors.push(format!("{}: {error}", cli_path.display())),
        }
    }

    Err(format!(
        "Unable to launch apm service. Set {CLI_PATH_ENV} to the apm CLI path. Tried {}",
        errors.join("; ")
    ))
}

fn cli_candidates() -> Vec<PathBuf> {
    cli_candidates_from(env::var(CLI_PATH_ENV).ok(), env::current_exe().ok())
}

fn cli_candidates_from(env_path: Option<String>, current_exe: Option<PathBuf>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = env_path {
        push_cli_candidate(&mut candidates, current_exe.as_deref(), PathBuf::from(path));
    }

    for candidate in bundled_cli_candidates(current_exe.as_deref()) {
        push_cli_candidate(&mut candidates, current_exe.as_deref(), candidate);
    }
    candidates.push(PathBuf::from("apm"));
    candidates
}

fn bundled_cli_candidates(current_exe: Option<&Path>) -> Vec<PathBuf> {
    let Some(exe) = current_exe else {
        return Vec::new();
    };
    let Some(parent) = exe.parent() else {
        return Vec::new();
    };

    vec![parent.join("apm-cli"), parent.join("apm")]
}

fn push_cli_candidate(
    candidates: &mut Vec<PathBuf>,
    current_exe: Option<&Path>,
    candidate: PathBuf,
) {
    if current_exe != Some(candidate.as_path()) {
        candidates.push(candidate);
    }
}

fn wait_for_ready_service(port: u16, child: &mut Child) -> Result<ReadyService, String> {
    let deadline = Instant::now() + SERVICE_STARTUP_TIMEOUT;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("Failed to inspect local service process: {error}"))?
        {
            return Err(format!("apm service exited before it was ready: {status}"));
        }

        match probe_ready_service(port) {
            Ok(ready) => return Ok(ready),
            Err(error) if Instant::now() >= deadline => {
                return Err(format!("Timed out waiting for apm service: {error}"));
            }
            Err(_) => {}
        }

        std::thread::sleep(Duration::from_millis(50));
    }
}

fn probe_ready_service(port: u16) -> Result<ReadyService, ServiceProbeError> {
    let health = probe_health(port)?;
    let contract = probe_contract(port)?;
    validate_ready_service(&health, &contract).map_err(ServiceProbeError::NotReady)?;
    if !token_available(Path::new(&health.auth.token_file)) {
        return Err(ServiceProbeError::NotReady(format!(
            "loopback token is not available at {}",
            health.auth.token_file
        )));
    }

    Ok(ReadyService { health, contract })
}

fn probe_health(port: u16) -> Result<ServiceHealth, ServiceProbeError> {
    probe_json(port, "/v1/health", "service health")
}

fn probe_contract(port: u16) -> Result<LocalServiceContract, ServiceProbeError> {
    probe_json(port, "/v1/service/contract", "service contract")
}

fn probe_json<T: DeserializeOwned>(
    port: u16,
    path: &str,
    label: &str,
) -> Result<T, ServiceProbeError> {
    let mut stream = TcpStream::connect_timeout(&service_addr(port), CONNECT_TIMEOUT)
        .map_err(|error| ServiceProbeError::Unreachable(error.to_string()))?;
    stream
        .set_read_timeout(Some(READ_TIMEOUT))
        .map_err(|error| {
            ServiceProbeError::NotReady(format!("failed to set service read timeout: {error}"))
        })?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).map_err(|error| {
        ServiceProbeError::NotReady(format!("failed to request {label}: {error}"))
    })?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| ServiceProbeError::NotReady(format!("failed to read {label}: {error}")))?;
    parse_json_response(&response, label)
}

fn parse_json_response<T: DeserializeOwned>(
    response: &str,
    label: &str,
) -> Result<T, ServiceProbeError> {
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| ServiceProbeError::NotReady(format!("{label} response was not HTTP")))?;
    if !head.starts_with("HTTP/1.1 200 OK") && !head.starts_with("HTTP/1.0 200 OK") {
        return Err(ServiceProbeError::NotReady(format!(
            "{label} returned {}",
            head.lines().next().unwrap_or("unknown status")
        )));
    }
    serde_json::from_str(body.trim()).map_err(|error| {
        ServiceProbeError::NotReady(format!("{label} was not valid JSON: {error}"))
    })
}

fn validate_ready_service(
    health: &ServiceHealth,
    contract: &LocalServiceContract,
) -> Result<(), String> {
    let expected = local_service_contract();

    if health.status != "ok" {
        return Err(format!("service health status {} is not ok", health.status));
    }
    if health.bind.host != expected.bind.host {
        return Err(format!(
            "apm service health host {} does not match desktop host {}",
            health.bind.host, expected.bind.host
        ));
    }
    if health.service_name != contract.service_name {
        return Err(format!(
            "service health name {} does not match contract name {}",
            health.service_name, contract.service_name
        ));
    }
    if health.api_version != contract.api_version {
        return Err(format!(
            "service health API version {} does not match contract API version {}",
            health.api_version, contract.api_version
        ));
    }
    if !health.auth.required || health.auth.header != LOOPBACK_TOKEN_HEADER {
        return Err(format!(
            "service auth header {} does not match desktop header {}",
            health.auth.header, LOOPBACK_TOKEN_HEADER
        ));
    }
    if contract.service_name != expected.service_name {
        return Err(format!(
            "apm service name {} does not match desktop service name {}",
            contract.service_name, expected.service_name
        ));
    }
    if contract.api_version != expected.api_version {
        return Err(format!(
            "apm service API version {} does not match desktop API version {}",
            contract.api_version, expected.api_version
        ));
    }
    if contract.schema_version != expected.schema_version {
        return Err(format!(
            "apm service contract schema {} does not match desktop contract schema {}",
            contract.schema_version, expected.schema_version
        ));
    }
    if !contract.security.localhost_only || contract.bind.host != expected.bind.host {
        return Err("apm service contract is not restricted to localhost".to_string());
    }
    if contract != &expected {
        return Err("apm service contract payload does not match desktop contract".to_string());
    }

    Ok(())
}

fn session_from_ready_service(
    status: DesktopServiceStatus,
    pid: Option<u32>,
    ready: ReadyService,
    message: String,
) -> DesktopServiceSession {
    let ReadyService { health, contract } = ready;
    DesktopServiceSession {
        status,
        url: service_url(health.bind.port),
        pid,
        api_version: contract.api_version,
        schema_version: contract.schema_version,
        token_header: health.auth.header,
        token_file: health.auth.token_file,
        token_available: true,
        privileged_install_policy: contract.security.privileged_install_policy,
        pending_runtime_work: contract.pending_runtime_work,
        message,
    }
}

fn not_started_session(bind: ServiceBind, exited: Option<ExitStatus>) -> DesktopServiceSession {
    let message = exited.map_or_else(
        || "No local apm service is running".to_string(),
        |status| format!("Local service exited: {status}"),
    );
    let contract = local_service_contract();
    DesktopServiceSession {
        status: DesktopServiceStatus::NotStarted,
        url: service_url(bind.port),
        pid: None,
        api_version: contract.api_version,
        schema_version: contract.schema_version,
        token_header: LOOPBACK_TOKEN_HEADER.to_string(),
        token_file: service_token_file(),
        token_available: false,
        privileged_install_policy: contract.security.privileged_install_policy,
        pending_runtime_work: contract.pending_runtime_work,
        message,
    }
}

fn unavailable_session(
    bind: ServiceBind,
    pid: Option<u32>,
    message: String,
) -> DesktopServiceSession {
    let contract = local_service_contract();
    DesktopServiceSession {
        status: DesktopServiceStatus::Unavailable,
        url: service_url(bind.port),
        pid,
        api_version: contract.api_version,
        schema_version: contract.schema_version,
        token_header: LOOPBACK_TOKEN_HEADER.to_string(),
        token_file: service_token_file(),
        token_available: false,
        privileged_install_policy: contract.security.privileged_install_policy,
        pending_runtime_work: contract.pending_runtime_work,
        message,
    }
}

fn service_bind() -> ServiceBind {
    let contract = local_service_contract();
    let port = env::var(&contract.bind.port_env)
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(contract.bind.default_port);
    ServiceBind { port }
}

fn service_token_file() -> String {
    config::init()
        .map(|config| loopback_token_file(&config))
        .unwrap_or_else(|_| loopback_token_file(&config::Config::default()))
        .display()
        .to_string()
}

fn service_addr(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

fn service_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

struct ServiceBind {
    port: u16,
}

struct ReadyService {
    health: ServiceHealth,
    contract: LocalServiceContract,
}

#[derive(Debug)]
enum ServiceProbeError {
    Unreachable(String),
    NotReady(String),
}

impl ServiceProbeError {
    fn is_unreachable(&self) -> bool {
        matches!(self, Self::Unreachable(_))
    }
}

impl fmt::Display for ServiceProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable(error) => write!(formatter, "service is not reachable: {error}"),
            Self::NotReady(error) => formatter.write_str(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apm_core::service::{service_health, RuntimeBind};

    #[test]
    fn service_url_uses_loopback_host() {
        assert_eq!(service_url(4767), "http://127.0.0.1:4767");
    }

    #[test]
    fn parse_json_response_rejects_non_ok_status() {
        let error = parse_json_response::<ServiceHealth>(
            "HTTP/1.1 503 Service Unavailable\r\n\r\n{}",
            "service health",
        )
        .expect_err("non-200 health should be rejected");

        assert!(error.to_string().contains("503 Service Unavailable"));
    }

    #[test]
    fn probe_error_keeps_unreachable_distinct_from_not_ready() {
        assert!(ServiceProbeError::Unreachable("refused".to_string()).is_unreachable());
        assert!(!ServiceProbeError::NotReady("wrong contract".to_string()).is_unreachable());
        assert_eq!(
            ServiceProbeError::Unreachable("refused".to_string()).to_string(),
            "service is not reachable: refused"
        );
    }

    #[test]
    fn validate_ready_service_accepts_expected_contract() {
        let health = matching_health();
        let contract = local_service_contract();

        validate_ready_service(&health, &contract).expect("matching service should be accepted");
    }

    #[test]
    fn service_sessions_include_pending_runtime_work() {
        let session = not_started_session(service_bind(), None);

        assert!(session
            .pending_runtime_work
            .iter()
            .any(|item| item.contains("release-channel artifact acceptance")));
        assert!(session
            .pending_runtime_work
            .iter()
            .any(|item| item.contains("native MLX/Core ML")));
    }

    #[test]
    fn validate_ready_service_rejects_schema_mismatch() {
        let health = matching_health();
        let mut contract = local_service_contract();
        contract.schema_version = "2026-06-01-old-contract".to_string();

        let error = validate_ready_service(&health, &contract)
            .expect_err("contract schema mismatch should be rejected");

        assert!(error.contains("contract schema 2026-06-01-old-contract"));
    }

    #[test]
    fn validate_ready_service_rejects_privileged_policy_mismatch() {
        let health = matching_health();
        let mut contract = local_service_contract();
        contract
            .security
            .privileged_install_policy
            .design
            .helper
            .bundle_identifier = "com.example.pkg-helper".to_string();

        let error = validate_ready_service(&health, &contract)
            .expect_err("privileged policy mismatch should be rejected");

        assert!(error.contains("contract payload"));
    }

    #[test]
    fn validate_ready_service_rejects_operation_control_policy_mismatch() {
        let health = matching_health();
        let mut contract = local_service_contract();
        contract.operation_control_policy.cancel_endpoint_id = "operation.stop".to_string();

        let error = validate_ready_service(&health, &contract)
            .expect_err("operation control policy mismatch should be rejected");

        assert!(error.contains("contract payload"));
    }

    #[test]
    fn validate_ready_service_rejects_recovery_policy_mismatch() {
        let health = matching_health();
        let mut contract = local_service_contract();
        contract
            .operation_recovery_policy
            .retry_all_ready_recovery_candidates = false;

        let error = validate_ready_service(&health, &contract)
            .expect_err("operation recovery policy mismatch should be rejected");

        assert!(error.contains("contract payload"));
    }

    #[test]
    fn validate_ready_service_rejects_endpoint_mismatch() {
        let health = matching_health();
        let mut contract = local_service_contract();
        contract
            .endpoints
            .retain(|endpoint| endpoint.id != "operation.cancel");

        let error = validate_ready_service(&health, &contract)
            .expect_err("endpoint mismatch should be rejected");

        assert!(error.contains("contract payload"));
    }

    #[test]
    fn validate_ready_service_rejects_event_stream_mismatch() {
        let health = matching_health();
        let mut contract = local_service_contract();
        contract.event_streams.clear();

        let error = validate_ready_service(&health, &contract)
            .expect_err("event stream mismatch should be rejected");

        assert!(error.contains("contract payload"));
    }

    #[test]
    fn validate_ready_service_rejects_non_loopback_health() {
        let mut health = matching_health();
        health.bind.host = "0.0.0.0".to_string();
        let contract = local_service_contract();

        let error = validate_ready_service(&health, &contract)
            .expect_err("non-loopback health should be rejected");

        assert!(error.contains("health host 0.0.0.0"));
    }

    #[test]
    fn validate_ready_service_rejects_auth_header_mismatch() {
        let mut health = matching_health();
        health.auth.header = "x-other-token".to_string();
        let contract = local_service_contract();

        let error = validate_ready_service(&health, &contract)
            .expect_err("auth header mismatch should be rejected");

        assert!(error.contains("auth header x-other-token"));
    }

    #[test]
    fn cli_candidates_prefer_env_then_bundled_sidecar_then_path() {
        let current = PathBuf::from("/Applications/apm.app/Contents/MacOS/apm");
        let candidates =
            cli_candidates_from(Some("/tmp/dev-apm".to_string()), Some(current.clone()));

        assert_eq!(candidates[0], PathBuf::from("/tmp/dev-apm"));
        assert!(candidates[1].ends_with("apm-cli"));
        assert!(!candidates.iter().any(|candidate| candidate == &current));
        assert_eq!(candidates.last(), Some(&PathBuf::from("apm")));
    }

    #[test]
    fn push_cli_candidate_skips_current_executable() {
        let current = PathBuf::from("/Applications/apm.app/Contents/MacOS/apm");
        let mut candidates = Vec::new();

        push_cli_candidate(&mut candidates, Some(&current), current.clone());
        push_cli_candidate(
            &mut candidates,
            Some(&current),
            PathBuf::from("/Applications/apm.app/Contents/MacOS/apm-cli"),
        );

        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].ends_with("apm-cli"));
    }

    fn matching_health() -> ServiceHealth {
        service_health(
            &config::Config::default(),
            RuntimeBind {
                host: "127.0.0.1".to_string(),
                port: 4767,
            },
        )
    }
}
