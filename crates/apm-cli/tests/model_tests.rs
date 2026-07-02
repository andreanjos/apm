mod support;

use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};
use support::{command, CliTestEnv};

#[test]
fn model_pull_downloads_weights_and_caches_manifest() {
    let env = CliTestEnv::new();
    let apm_home = assert_fs::TempDir::new().expect("apm home");
    let manifest_dir = assert_fs::TempDir::new().expect("manifest dir");
    let weights = b"model weights".to_vec();
    let sha256 = sha256_hex(&weights);
    let server = serve_once(weights);
    let manifest_path = manifest_dir.path().join("test-model.toml");
    fs::write(&manifest_path, test_manifest(&server.url, &sha256)).expect("write manifest");

    let output = command(&env)
        .env("APM_HOME", apm_home.path())
        .args(["--json", "model", "pull"])
        .arg(&manifest_path)
        .output()
        .expect("run apm model pull");

    assert!(
        output.status.success(),
        "model pull should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    server.join();
    let result: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("model pull JSON");
    assert_eq!(result["package"], "test-model@1.0.0");
    assert_eq!(result["status"], "pulled");
    assert_eq!(result["sha256"], sha256);
    assert_eq!(result["bytes"], 13);
    assert!(apm_home.path().join(format!("weights/{sha256}")).exists());
    assert!(apm_home
        .path()
        .join("manifests/test-model/1.0.0.toml")
        .exists());

    let cached = command(&env)
        .env("APM_HOME", apm_home.path())
        .args(["--json", "model", "pull"])
        .arg(&manifest_path)
        .output()
        .expect("run cached apm model pull");

    assert!(
        cached.status.success(),
        "cached model pull should succeed; stderr: {}",
        String::from_utf8_lossy(&cached.stderr)
    );
    let cached_result: serde_json::Value =
        serde_json::from_slice(&cached.stdout).expect("cached model pull JSON");
    assert_eq!(cached_result["status"], "cached");
}

#[test]
fn model_search_available_and_pull_resolve_registry_model() {
    let env = CliTestEnv::new();
    let apm_home = assert_fs::TempDir::new().expect("apm home");
    let registry = assert_fs::TempDir::new().expect("model registry");
    let weights = b"registry model".to_vec();
    let sha256 = sha256_hex(&weights);
    let server = serve_once(weights);
    write_test_config(&env, registry.path());
    write_registry_model(registry.path(), &server.url, &sha256);

    let search = command(&env)
        .env("APM_HOME", apm_home.path())
        .args(["--json", "model", "search", "--available", "stems"])
        .output()
        .expect("run available model search");

    assert!(
        search.status.success(),
        "available model search should succeed; stderr: {}",
        String::from_utf8_lossy(&search.stderr)
    );
    let search_json: serde_json::Value =
        serde_json::from_slice(&search.stdout).expect("available model search JSON");
    assert_eq!(search_json["packages"][0]["package"], "test-model@1.0.0");

    let pull = command(&env)
        .env("APM_HOME", apm_home.path())
        .args(["--json", "model", "pull", "test-model@1.0.0"])
        .output()
        .expect("run registry model pull");

    assert!(
        pull.status.success(),
        "registry model pull should succeed; stderr: {}",
        String::from_utf8_lossy(&pull.stderr)
    );
    server.join();
    let pull_json: serde_json::Value =
        serde_json::from_slice(&pull.stdout).expect("registry model pull JSON");
    assert_eq!(pull_json["package"], "test-model@1.0.0");
    assert_eq!(pull_json["status"], "pulled");
    assert!(apm_home
        .path()
        .join("manifests/test-model/1.0.0.toml")
        .exists());
    assert!(apm_home.path().join(format!("weights/{sha256}")).exists());
}

#[test]
fn model_list_and_rm_use_cached_model_store() {
    let env = CliTestEnv::new();
    let apm_home = assert_fs::TempDir::new().expect("apm home");
    let weights = b"model weights";
    let sha256 = sha256_hex(weights);
    let manifest_dir = apm_home.path().join("manifests/test-model");
    fs::create_dir_all(&manifest_dir).expect("create cached manifest dir");
    fs::create_dir_all(apm_home.path().join("weights")).expect("create weights dir");
    fs::write(
        manifest_dir.join("1.0.0.toml"),
        test_manifest("https://example.test/model.safetensors", &sha256),
    )
    .expect("write cached manifest");
    fs::write(apm_home.path().join(format!("weights/{sha256}")), weights)
        .expect("write cached weights");

    let list = command(&env)
        .env("APM_HOME", apm_home.path())
        .args(["--json", "model", "list"])
        .output()
        .expect("run apm model list");

    assert!(
        list.status.success(),
        "model list should succeed; stderr: {}",
        String::from_utf8_lossy(&list.stderr)
    );
    let list_json: serde_json::Value =
        serde_json::from_slice(&list.stdout).expect("model list JSON");
    assert_eq!(list_json["packages"][0]["package"], "test-model@1.0.0");
    assert_eq!(list_json["packages"][0]["weights_cached"], true);

    let search = command(&env)
        .env("APM_HOME", apm_home.path())
        .args(["--json", "model", "search", "stems"])
        .output()
        .expect("run apm model search");

    assert!(
        search.status.success(),
        "model search should succeed; stderr: {}",
        String::from_utf8_lossy(&search.stderr)
    );
    let search_json: serde_json::Value =
        serde_json::from_slice(&search.stdout).expect("model search JSON");
    assert_eq!(search_json["packages"][0]["package"], "test-model@1.0.0");

    let empty_search = command(&env)
        .env("APM_HOME", apm_home.path())
        .args(["--json", "model", "search", "whisper"])
        .output()
        .expect("run empty apm model search");
    let empty_json: serde_json::Value =
        serde_json::from_slice(&empty_search.stdout).expect("empty model search JSON");
    assert_eq!(
        empty_json["packages"]
            .as_array()
            .expect("packages array")
            .len(),
        0
    );

    let install = command(&env)
        .env("APM_HOME", apm_home.path())
        .args(["--json", "model", "install", "test-model@1.0.0"])
        .output()
        .expect("run apm model install");

    assert!(
        install.status.success(),
        "model install should succeed; stderr: {}",
        String::from_utf8_lossy(&install.stderr)
    );
    let install_json: serde_json::Value =
        serde_json::from_slice(&install.stdout).expect("model install JSON");
    assert_eq!(install_json["package_id"], "test-model@1.0.0");
    assert_eq!(install_json["runtime_mode"], "native-mlx");
    assert_eq!(install_json["runtime"]["adapter"], "native-mlx");
    assert_eq!(install_json["runtime"]["status"], "prepared");
    assert!(install_json["runtime"]["runtime_dir"]
        .as_str()
        .expect("runtime dir")
        .ends_with("runtimes/native-mlx/test-model/1.0.0"));
    assert_eq!(install_json["weights"]["status"], "cached");
    assert_eq!(install_json["weights"]["sha256"], sha256);
    assert!(apm_home
        .path()
        .join("runtimes/native-mlx/test-model/1.0.0/adapter.toml")
        .exists());

    let run = command(&env)
        .env("APM_HOME", apm_home.path())
        .args([
            "--json",
            "model",
            "run",
            "test-model@1.0.0",
            "--input",
            "mix.wav",
            "--output",
            "stems/",
            "--param",
            "stems=2",
            "--param",
            "shifts=3",
        ])
        .output()
        .expect("run apm model run");

    assert!(
        run.status.success(),
        "model run should return blocked result; stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let run_json: serde_json::Value = serde_json::from_slice(&run.stdout).expect("model run JSON");
    assert_eq!(run_json["package_id"], "test-model@1.0.0");
    assert_eq!(run_json["status"], "blocked");
    assert_eq!(run_json["plan"]["status"], "planned");
    assert_eq!(run_json["plan"]["runtime_mode"], "native-mlx");
    assert_eq!(run_json["plan"]["adapter"], "native-mlx");
    assert_eq!(run_json["plan"]["runtime_entry"], "test.Model");
    assert_eq!(run_json["plan"]["input_path"], "mix.wav");
    assert_eq!(run_json["plan"]["output_path"], "stems/");
    assert_eq!(run_json["plan"]["execution"]["status"], "blocked");
    assert_eq!(
        run_json["plan"]["execution"]["blocker"],
        "adapter_runner_unavailable"
    );
    assert!(run_json["plan"]["execution"]["message"]
        .as_str()
        .expect("execution message")
        .contains("native-mlx execution"));
    assert_eq!(run_json["plan"]["params"][0]["name"], "stems");
    assert_eq!(run_json["plan"]["params"][0]["value"], "2");
    assert_eq!(run_json["plan"]["params"][0]["source"], "request");
    assert_eq!(run_json["plan"]["params"][1]["name"], "shifts");
    assert_eq!(run_json["plan"]["params"][1]["value"], 3);
    assert_eq!(run_json["plan"]["params"][1]["source"], "request");
    assert!(run_json["plan"]["adapter_manifest_path"]
        .as_str()
        .expect("adapter manifest path")
        .ends_with("runtimes/native-mlx/test-model/1.0.0/adapter.toml"));
    assert!(run_json["message"]
        .as_str()
        .expect("message")
        .contains("native-mlx execution"));

    let removed = command(&env)
        .env("APM_HOME", apm_home.path())
        .args(["--json", "model", "rm", "test-model@1.0.0"])
        .output()
        .expect("run apm model rm");

    assert!(
        removed.status.success(),
        "model rm should succeed; stderr: {}",
        String::from_utf8_lossy(&removed.stderr)
    );
    let removed_json: serde_json::Value =
        serde_json::from_slice(&removed.stdout).expect("model rm JSON");
    assert_eq!(removed_json["package"], "test-model@1.0.0");
    assert_eq!(removed_json["status"], "removed");
    assert_eq!(removed_json["removed_manifest"], true);
    assert_eq!(removed_json["removed_runtime"], true);
    assert_eq!(removed_json["removed_weight"], true);
    assert!(!manifest_dir.join("1.0.0.toml").exists());
    assert!(!apm_home
        .path()
        .join("runtimes/native-mlx/test-model/1.0.0")
        .exists());
    assert!(!apm_home.path().join(format!("weights/{sha256}")).exists());
}

fn serve_once(body: Vec<u8>) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let url = format!(
        "http://{}/model.safetensors",
        listener.local_addr().expect("addr")
    );
    let handle = thread::spawn(move || {
        listener
            .set_nonblocking(true)
            .expect("set nonblocking listener");
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "timed out waiting for request");
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept request: {error}"),
            }
        };
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

struct TestServer {
    url: String,
    handle: thread::JoinHandle<()>,
}

impl TestServer {
    fn join(self) {
        self.handle.join().expect("server thread should finish");
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn write_test_config(env: &CliTestEnv, registry_path: &std::path::Path) {
    let config_dir = env.config_home.path().join("apm");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(
        config_dir.join("config.toml"),
        format!("default_registry_url = \"{}\"\n", registry_path.display()),
    )
    .expect("write config");
}

fn write_registry_model(registry_path: &std::path::Path, source: &str, sha256: &str) {
    let manifest_path = registry_path.join("models/test-model/1.0.0.toml");
    fs::create_dir_all(manifest_path.parent().expect("manifest parent"))
        .expect("create registry model dir");
    fs::write(manifest_path, test_manifest(source, sha256)).expect("write registry model");
}

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

[[params]]
name = "stems"
type = "enum"
values = ["2", "4", "6"]
default = "4"

[[params]]
name = "shifts"
type = "int"
min = 1
max = 10
default = 1

[license]
spdx = "MIT"
commercial = true

[hardware]
min_memory_gb = 8
"#
    )
}
