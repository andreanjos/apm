use std::fmt;

#[cfg(feature = "reqwest")]
use anyhow::Context;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::cancel::{ensure_not_cancelled, CancellationToken};
#[cfg(feature = "reqwest")]
use crate::install;

use super::{ModelManifest, ModelStore};

#[cfg(feature = "reqwest")]
const MODEL_WEIGHT_PROGRESS_STEP_BYTES: u64 = 25 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelWeightPullResult {
    pub package_id: String,
    pub source: String,
    pub resolved_url: String,
    pub sha256: String,
    pub path: String,
    pub bytes: u64,
    pub status: ModelWeightPullStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelWeightPullStatus {
    Cached,
    Pulled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelWeightPullProgress {
    pub bytes: u64,
    pub total_bytes: Option<u64>,
}

pub trait ModelWeightPullObserver: CancellationToken {
    fn progress(&mut self, _progress: ModelWeightPullProgress) {}
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopModelWeightPullObserver;

impl CancellationToken for NoopModelWeightPullObserver {}

impl ModelWeightPullObserver for NoopModelWeightPullObserver {}

impl fmt::Display for ModelWeightPullStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cached => f.write_str("cached"),
            Self::Pulled => f.write_str("pulled"),
        }
    }
}

#[cfg(feature = "reqwest")]
pub fn pull_model_weights(
    store: &ModelStore,
    manifest: &ModelManifest,
) -> Result<ModelWeightPullResult> {
    let mut observer = NoopModelWeightPullObserver;
    pull_model_weights_with_observer(store, manifest, &mut observer)
}

#[cfg(feature = "reqwest")]
pub fn pull_model_weights_with_cancellation(
    store: &ModelStore,
    manifest: &ModelManifest,
    cancellation: &(impl CancellationToken + ?Sized),
) -> Result<ModelWeightPullResult> {
    let mut observer = CancellationOnlyModelWeightPullObserver { cancellation };
    pull_model_weights_with_observer(store, manifest, &mut observer)
}

#[cfg(feature = "reqwest")]
pub fn pull_model_weights_with_observer(
    store: &ModelStore,
    manifest: &ModelManifest,
    observer: &mut (impl ModelWeightPullObserver + ?Sized),
) -> Result<ModelWeightPullResult> {
    ensure_not_cancelled(observer)?;
    let target = store.weight_path(&manifest.weights.sha256);
    if cached_weight_is_valid(&target, &manifest.weights.sha256) {
        return weight_pull_result(
            store,
            manifest,
            manifest.weights.source.clone(),
            ModelWeightPullStatus::Cached,
        );
    }

    let source = ModelWeightSource::parse(&manifest.weights.source)?;
    let resolved_url = source.url();
    ensure_not_cancelled(observer)?;
    remove_weight_artifacts(&target);
    let result = crate::download::download_to_path(
        &resolved_url,
        &target,
        crate::download::DownloadOptions {
            progress_step_bytes: MODEL_WEIGHT_PROGRESS_STEP_BYTES,
        },
        ModelWeightDownloadObserver { observer },
    )
    .and_then(|_| {
        ensure_not_cancelled(observer)?;
        install::verify_file_sha256(&target, &manifest.weights.sha256).map(|_| ())
    });

    if let Err(error) = result {
        remove_weight_artifacts(&target);
        return Err(error);
    }

    weight_pull_result(store, manifest, resolved_url, ModelWeightPullStatus::Pulled)
}

#[cfg(not(feature = "reqwest"))]
pub fn pull_model_weights(
    _store: &ModelStore,
    _manifest: &ModelManifest,
) -> Result<ModelWeightPullResult> {
    bail!("model weight pulling requires the apm-core reqwest feature");
}

#[cfg(not(feature = "reqwest"))]
pub fn pull_model_weights_with_cancellation(
    _store: &ModelStore,
    _manifest: &ModelManifest,
    cancellation: &(impl CancellationToken + ?Sized),
) -> Result<ModelWeightPullResult> {
    ensure_not_cancelled(cancellation)?;
    bail!("model weight pulling requires the apm-core reqwest feature");
}

#[cfg(not(feature = "reqwest"))]
pub fn pull_model_weights_with_observer(
    _store: &ModelStore,
    _manifest: &ModelManifest,
    observer: &mut (impl ModelWeightPullObserver + ?Sized),
) -> Result<ModelWeightPullResult> {
    ensure_not_cancelled(observer)?;
    bail!("model weight pulling requires the apm-core reqwest feature");
}

#[cfg(feature = "reqwest")]
struct CancellationOnlyModelWeightPullObserver<'a, T: CancellationToken + ?Sized> {
    cancellation: &'a T,
}

#[cfg(feature = "reqwest")]
impl<T: CancellationToken + ?Sized> CancellationToken
    for CancellationOnlyModelWeightPullObserver<'_, T>
{
    fn cancel_requested(&self) -> bool {
        self.cancellation.cancel_requested()
    }
}

#[cfg(feature = "reqwest")]
impl<T: CancellationToken + ?Sized> ModelWeightPullObserver
    for CancellationOnlyModelWeightPullObserver<'_, T>
{
}

#[cfg(feature = "reqwest")]
struct ModelWeightDownloadObserver<'a, T: ModelWeightPullObserver + ?Sized> {
    observer: &'a mut T,
}

#[cfg(feature = "reqwest")]
impl<T: ModelWeightPullObserver + ?Sized> crate::download::DownloadObserver
    for ModelWeightDownloadObserver<'_, T>
{
    fn checkpoint(&mut self) -> Result<()> {
        ensure_not_cancelled(self.observer)
    }

    fn progress(&mut self, progress: crate::download::DownloadProgress) {
        self.observer.progress(ModelWeightPullProgress {
            bytes: progress.bytes,
            total_bytes: progress.total_bytes,
        });
    }
}

#[cfg(any(test, feature = "reqwest"))]
#[derive(Debug, Clone, PartialEq, Eq)]
enum ModelWeightSource {
    Http(String),
    HuggingFace {
        namespace: String,
        repository: String,
        revision: String,
        path: String,
    },
}

#[cfg(any(test, feature = "reqwest"))]
impl ModelWeightSource {
    fn parse(source: &str) -> Result<Self> {
        let source = source.trim();
        if source.starts_with("http://") || source.starts_with("https://") {
            return Ok(Self::Http(source.to_string()));
        }
        if let Some(value) = source.strip_prefix("hf:") {
            return parse_hugging_face_source(value);
        }
        bail!(
            "weights.source must be an http(s) URL or an explicit Hugging Face file source like hf:org/repo/path/to/model.safetensors"
        );
    }

    fn url(&self) -> String {
        match self {
            Self::Http(url) => url.clone(),
            Self::HuggingFace {
                namespace,
                repository,
                revision,
                path,
            } => {
                format!("https://huggingface.co/{namespace}/{repository}/resolve/{revision}/{path}")
            }
        }
    }
}

#[cfg(any(test, feature = "reqwest"))]
fn parse_hugging_face_source(source: &str) -> Result<ModelWeightSource> {
    let segments = source.split('/').collect::<Vec<_>>();
    if segments.len() < 3 || segments.iter().any(|segment| segment.trim().is_empty()) {
        bail!(
            "hf weights.source must include namespace, repository, and file path, e.g. hf:org/repo/model.safetensors"
        );
    }

    let namespace = safe_hf_segment("namespace", segments[0])?;
    let (repository, revision) = parse_hf_repository_segment(segments[1])?;
    let path = segments[2..].join("/");
    ensure_safe_hf_path(&path)?;

    Ok(ModelWeightSource::HuggingFace {
        namespace,
        repository,
        revision,
        path,
    })
}

#[cfg(any(test, feature = "reqwest"))]
fn parse_hf_repository_segment(segment: &str) -> Result<(String, String)> {
    let (repository, revision) = segment.split_once('@').unwrap_or((segment, "main"));
    Ok((
        safe_hf_segment("repository", repository)?,
        safe_hf_segment("revision", revision)?,
    ))
}

#[cfg(any(test, feature = "reqwest"))]
fn safe_hf_segment(field: &str, value: &str) -> Result<String> {
    let safe = !value.is_empty()
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if safe {
        Ok(value.to_string())
    } else {
        bail!("hf weights.source {field} contains unsupported characters");
    }
}

#[cfg(any(test, feature = "reqwest"))]
fn ensure_safe_hf_path(path: &str) -> Result<()> {
    let safe = !path.is_empty()
        && path.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        });
    if safe {
        Ok(())
    } else {
        bail!("hf weights.source file path contains unsupported characters");
    }
}

#[cfg(feature = "reqwest")]
fn weight_pull_result(
    store: &ModelStore,
    manifest: &ModelManifest,
    resolved_url: String,
    status: ModelWeightPullStatus,
) -> Result<ModelWeightPullResult> {
    let path = store.weight_path(&manifest.weights.sha256);
    let bytes = path
        .metadata()
        .with_context(|| format!("Cannot stat cached model weights: {}", path.display()))?
        .len();
    Ok(ModelWeightPullResult {
        package_id: manifest.package_id(),
        source: manifest.weights.source.clone(),
        resolved_url,
        sha256: manifest.weights.sha256.clone(),
        path: path.display().to_string(),
        bytes,
        status,
    })
}

#[cfg(feature = "reqwest")]
fn cached_weight_is_valid(path: &std::path::Path, expected_sha256: &str) -> bool {
    path.exists() && install::verify_file_sha256(path, expected_sha256).is_ok()
}

#[cfg(feature = "reqwest")]
fn remove_weight_artifacts(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(crate::download::part_path(path));
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "reqwest")]
    use std::cell::Cell;
    #[cfg(feature = "reqwest")]
    use std::io::{Read, Write};
    #[cfg(feature = "reqwest")]
    use std::net::TcpListener;
    #[cfg(feature = "reqwest")]
    use std::thread;

    #[cfg(feature = "reqwest")]
    use sha2::{Digest, Sha256};

    #[cfg(feature = "reqwest")]
    use crate::{cancel::CancellationToken, ApmError};

    use super::*;

    #[test]
    fn resolves_direct_http_weight_source() {
        let source =
            ModelWeightSource::parse("https://example.test/model.safetensors").expect("source");

        assert_eq!(source.url(), "https://example.test/model.safetensors");
    }

    #[test]
    fn resolves_explicit_hugging_face_weight_source() {
        let source =
            ModelWeightSource::parse("hf:mlx-community/demucs-mlx-fp16@main/model.safetensors")
                .expect("source");

        assert_eq!(
            source.url(),
            "https://huggingface.co/mlx-community/demucs-mlx-fp16/resolve/main/model.safetensors"
        );
    }

    #[test]
    fn rejects_repo_only_hugging_face_weight_source() {
        let error = ModelWeightSource::parse("hf:mlx-community/demucs-mlx-fp16")
            .expect_err("repo-only source should fail");

        assert!(error.to_string().contains("must include namespace"));
    }

    #[cfg(feature = "reqwest")]
    #[test]
    fn pulls_and_reuses_direct_http_weights() {
        let bytes = b"model weights".to_vec();
        let sha256 = sha256_hex(&bytes);
        let server = serve_once(bytes);
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        let manifest =
            ModelManifest::from_toml_str(&test_manifest(&server.url, &sha256)).expect("manifest");

        let pulled = pull_model_weights(&store, &manifest).expect("pull weights");
        let cached = pull_model_weights(&store, &manifest).expect("reuse weights");
        server.join();

        assert_eq!(pulled.status, ModelWeightPullStatus::Pulled);
        assert_eq!(pulled.bytes, 13);
        assert_eq!(cached.status, ModelWeightPullStatus::Cached);
        assert_eq!(
            std::fs::read(store.weight_path(&sha256)).expect("read cached weights"),
            b"model weights"
        );
    }

    #[cfg(feature = "reqwest")]
    #[test]
    fn reports_weight_download_progress() {
        let bytes = b"model weights".to_vec();
        let sha256 = sha256_hex(&bytes);
        let server = serve_once(bytes);
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        let manifest =
            ModelManifest::from_toml_str(&test_manifest(&server.url, &sha256)).expect("manifest");
        let mut observer = RecordingObserver::default();

        pull_model_weights_with_observer(&store, &manifest, &mut observer).expect("pull weights");
        server.join();

        assert_eq!(
            observer.progress.as_slice(),
            &[ModelWeightPullProgress {
                bytes: 13,
                total_bytes: Some(13)
            }]
        );
    }

    #[cfg(feature = "reqwest")]
    #[test]
    fn deletes_bad_checksum_download() {
        let bytes = b"model weights".to_vec();
        let server = serve_once(bytes);
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        let manifest = ModelManifest::from_toml_str(&test_manifest(
            &server.url,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ))
        .expect("manifest");

        let error = pull_model_weights(&store, &manifest).expect_err("checksum should fail");
        server.join();

        assert!(error.to_string().contains("SHA256 checksum mismatch"));
        assert!(!store.weight_path(&manifest.weights.sha256).exists());
    }

    #[cfg(feature = "reqwest")]
    #[test]
    fn cancellation_before_weight_pull_returns_operation_canceled() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        let manifest = ModelManifest::from_toml_str(&test_manifest(
            "https://example.test/model.safetensors",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ))
        .expect("manifest");

        let error = pull_model_weights_with_cancellation(&store, &manifest, &AlwaysCanceled)
            .expect_err("canceled pull should fail");

        assert_operation_canceled(&error);
        assert!(!store.weight_path(&manifest.weights.sha256).exists());
    }

    #[cfg(feature = "reqwest")]
    #[test]
    fn cancellation_during_weight_download_removes_partial_file() {
        let bytes = b"model weights".to_vec();
        let sha256 = sha256_hex(&bytes);
        let server = serve_once(bytes);
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path());
        let manifest =
            ModelManifest::from_toml_str(&test_manifest(&server.url, &sha256)).expect("manifest");
        let cancellation = CancelAfterChecks::new(3);

        let error = pull_model_weights_with_cancellation(&store, &manifest, &cancellation)
            .expect_err("canceled pull should fail");
        server.join();

        assert_operation_canceled(&error);
        let weight_path = store.weight_path(&sha256);
        assert!(!weight_path.exists());
        assert!(!crate::download::part_path(&weight_path).exists());
    }

    #[cfg(feature = "reqwest")]
    fn serve_once(body: Vec<u8>) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let url = format!(
            "http://{}/model.safetensors",
            listener.local_addr().expect("addr")
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

    #[cfg(feature = "reqwest")]
    struct TestServer {
        url: String,
        handle: thread::JoinHandle<()>,
    }

    #[cfg(feature = "reqwest")]
    impl TestServer {
        fn join(self) {
            self.handle.join().expect("server thread should finish");
        }
    }

    #[cfg(feature = "reqwest")]
    fn sha256_hex(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hex::encode(hasher.finalize())
    }

    #[cfg(feature = "reqwest")]
    struct AlwaysCanceled;

    #[cfg(feature = "reqwest")]
    impl CancellationToken for AlwaysCanceled {
        fn cancel_requested(&self) -> bool {
            true
        }
    }

    #[cfg(feature = "reqwest")]
    struct CancelAfterChecks {
        checks: Cell<usize>,
        cancel_after: usize,
    }

    #[cfg(feature = "reqwest")]
    #[derive(Default)]
    struct RecordingObserver {
        progress: Vec<ModelWeightPullProgress>,
    }

    #[cfg(feature = "reqwest")]
    impl CancellationToken for RecordingObserver {}

    #[cfg(feature = "reqwest")]
    impl ModelWeightPullObserver for RecordingObserver {
        fn progress(&mut self, progress: ModelWeightPullProgress) {
            self.progress.push(progress);
        }
    }

    #[cfg(feature = "reqwest")]
    impl CancelAfterChecks {
        fn new(cancel_after: usize) -> Self {
            Self {
                checks: Cell::new(0),
                cancel_after,
            }
        }
    }

    #[cfg(feature = "reqwest")]
    impl CancellationToken for CancelAfterChecks {
        fn cancel_requested(&self) -> bool {
            let checks = self.checks.get() + 1;
            self.checks.set(checks);
            checks > self.cancel_after
        }
    }

    #[cfg(feature = "reqwest")]
    fn assert_operation_canceled(error: &anyhow::Error) {
        assert!(matches!(
            error.downcast_ref::<ApmError>(),
            Some(ApmError::OperationCanceled)
        ));
    }

    #[cfg(feature = "reqwest")]
    fn test_manifest(source: &str, sha256: &str) -> String {
        format!(
            r#"
[package]
name = "test-model"
version = "1.0.0"
description = "Test model"
publisher = "apm-core"

[runtime]
mode = "native-mlx"
entry = "test.Model"

[weights]
source = "{source}"
sha256 = "{sha256}"
format = "safetensors"

[io]
input = "audio"
output = "stems"

[license]
spdx = "MIT"
commercial = true

[hardware]
min_memory_gb = 8
"#
        )
    }
}
