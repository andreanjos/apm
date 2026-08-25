use crate::model::ModelStore;

use super::{display_path, is_writable, DiagnosticCheck};

pub(super) const MODEL_STORE_CHECK_NAME: &str = "Model store";

pub(super) fn check_model_store() -> DiagnosticCheck {
    check_model_store_at(&ModelStore::default())
}

fn check_model_store_at(store: &ModelStore) -> DiagnosticCheck {
    let root = store.root();
    if !root.exists() {
        return DiagnosticCheck::warning(
            MODEL_STORE_CHECK_NAME,
            format!("not initialized: {}", display_path(root)),
            "Run `apm model store --init` to initialize the model store.",
        );
    }

    if !root.is_dir() {
        return DiagnosticCheck::failure(
            MODEL_STORE_CHECK_NAME,
            format!(
                "model store root is not a directory: {}",
                display_path(root)
            ),
            "Move or remove the file at the model store root, then initialize the store again.",
        );
    }

    if std::fs::read_dir(root).is_err() {
        return DiagnosticCheck::failure(
            MODEL_STORE_CHECK_NAME,
            format!("model store root is not readable: {}", display_path(root)),
            "Check model store permissions before importing manifests or pulling weights.",
        );
    }

    let mut missing = Vec::new();
    let mut invalid = Vec::new();
    let mut read_only = Vec::new();

    for (label, path) in [
        ("manifests", store.manifests_dir()),
        ("weights", store.weights_dir()),
        ("runtimes", store.runtimes_dir()),
        ("cache", store.cache_dir()),
        ("logs", store.logs_dir()),
    ] {
        if !path.exists() {
            missing.push(label.to_string());
            continue;
        }
        if !path.is_dir() {
            invalid.push(format!("{label} is not a directory"));
            continue;
        }
        if std::fs::read_dir(&path).is_err() {
            invalid.push(format!("{label} is not readable"));
            continue;
        }
        if !is_writable(&path) {
            read_only.push(label.to_string());
        }
    }

    if !invalid.is_empty() {
        return DiagnosticCheck::failure(
            MODEL_STORE_CHECK_NAME,
            invalid.join(", "),
            "Fix or remove invalid model store paths, then initialize the store again.",
        );
    }

    let config_file = store.config_file();
    if config_file.exists() && !config_file.is_file() {
        return DiagnosticCheck::failure(
            MODEL_STORE_CHECK_NAME,
            format!(
                "model store config path is not a file: {}",
                display_path(&config_file)
            ),
            "Move or remove the path so future model-store config can be written safely.",
        );
    }

    if !missing.is_empty() {
        return DiagnosticCheck::warning(
            MODEL_STORE_CHECK_NAME,
            format!("missing directories: {}", missing.join(", ")),
            "Run `apm model store --init` to restore the model store layout.",
        );
    }

    if !read_only.is_empty() {
        return DiagnosticCheck::warning(
            MODEL_STORE_CHECK_NAME,
            format!("read-only directories: {}", read_only.join(", ")),
            "Fix permissions before importing manifests, pulling weights, or preparing runtimes.",
        );
    }

    DiagnosticCheck::ok(
        MODEL_STORE_CHECK_NAME,
        format!("directories ready: {}", display_path(root)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::DiagnosticStatus;

    #[test]
    fn model_store_warns_without_creating_missing_root() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path().join(".apm"));

        let check = check_model_store_at(&store);

        assert_eq!(check.name, MODEL_STORE_CHECK_NAME);
        assert_eq!(check.status, DiagnosticStatus::Warning);
        assert!(check.detail.contains("not initialized"));
        assert!(!store.root().exists());
    }

    #[test]
    fn model_store_ok_when_expected_directories_exist() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path().join(".apm"));
        store.ensure().expect("initialize store");

        let check = check_model_store_at(&store);

        assert_eq!(check.status, DiagnosticStatus::Ok);
        assert!(check.detail.contains("directories ready"));
    }

    #[test]
    fn model_store_warns_when_expected_directories_are_missing() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path().join(".apm"));
        std::fs::create_dir_all(store.root()).expect("create root");

        let check = check_model_store_at(&store);

        assert_eq!(check.status, DiagnosticStatus::Warning);
        assert!(check.detail.contains("missing directories"));
        assert!(check.detail.contains("manifests"));
        assert!(check.hint.expect("hint").contains("model store --init"));
    }

    #[test]
    fn model_store_fails_when_expected_directory_is_a_file() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = ModelStore::new(temp.path().join(".apm"));
        std::fs::create_dir_all(store.root()).expect("create root");
        std::fs::write(store.weights_dir(), "not a dir").expect("write file");

        let check = check_model_store_at(&store);

        assert_eq!(check.status, DiagnosticStatus::Failure);
        assert!(check.detail.contains("weights is not a directory"));
    }
}
