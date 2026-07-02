use std::path::Path;

use crate::service::{
    privileged_install_policy, PRIVILEGED_HELPER_BUNDLE_IDENTIFIER, PRIVILEGED_HELPER_INSTALL_PATH,
    PRIVILEGED_HELPER_LAUNCHD_PLIST_PATH,
};

use super::{display_path, DiagnosticCheck};

pub(super) fn check_privileged_helper_artifacts() -> DiagnosticCheck {
    check_privileged_helper_artifacts_at(
        Path::new(PRIVILEGED_HELPER_INSTALL_PATH),
        Path::new(PRIVILEGED_HELPER_LAUNCHD_PLIST_PATH),
        privileged_install_policy().runs_pkg_installers,
    )
}

fn check_privileged_helper_artifacts_at(
    helper_path: &Path,
    launchd_plist_path: &Path,
    runs_pkg_installers: bool,
) -> DiagnosticCheck {
    let artifacts = [
        ("helper", helper_path),
        ("launchd plist", launchd_plist_path),
    ];
    let present = artifacts
        .iter()
        .filter_map(|(label, path)| {
            path.exists()
                .then(|| format!("{label} at {}", display_path(path)))
        })
        .collect::<Vec<_>>();

    if runs_pkg_installers {
        return enabled_helper_artifact_check(artifacts.len(), present.len());
    }

    disabled_helper_artifact_check(present)
}

fn enabled_helper_artifact_check(artifact_count: usize, present_count: usize) -> DiagnosticCheck {
    if present_count == artifact_count {
        return DiagnosticCheck::ok(
            "Privileged helper artifacts",
            format!("{PRIVILEGED_HELPER_BUNDLE_IDENTIFIER} helper and launchd plist present"),
        );
    }

    let missing_count = artifact_count - present_count;
    DiagnosticCheck::failure(
        "Privileged helper artifacts",
        format!(
            "privileged installer execution is enabled but {missing_count} artifact{} missing",
            if missing_count == 1 { "" } else { "s" }
        ),
        "Reinstall the signed apm app/helper pair before running privileged package installs.",
    )
}

fn disabled_helper_artifact_check(present: Vec<String>) -> DiagnosticCheck {
    if present.is_empty() {
        return DiagnosticCheck::ok(
            "Privileged helper artifacts",
            "no apm privileged helper artifacts installed",
        );
    }

    DiagnosticCheck::warning(
        "Privileged helper artifacts",
        format!(
            "unexpected artifact{} while PKG execution is disabled: {}",
            if present.len() == 1 { "" } else { "s" },
            present.join(", ")
        ),
        "Current builds should use external PKG handoff only. Confirm no helper-enabled apm build is installed before removing stale helper artifacts.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::DiagnosticStatus;

    #[test]
    fn helper_artifacts_ok_when_absent_and_execution_disabled() {
        let env = tempfile::tempdir().expect("temp dir");

        let check = check_privileged_helper_artifacts_at(
            &env.path().join("missing-helper"),
            &env.path().join("missing.plist"),
            false,
        );

        assert_eq!(check.name, "Privileged helper artifacts");
        assert_eq!(check.status, DiagnosticStatus::Ok);
        assert!(check.detail.contains("no apm privileged helper"));
    }

    #[test]
    fn helper_artifacts_warn_when_present_and_execution_disabled() {
        let env = tempfile::tempdir().expect("temp dir");
        let helper = env.path().join("com.apm.pkg-helper");
        let plist = env.path().join("com.apm.pkg-helper.plist");
        std::fs::write(&helper, "helper").expect("write helper");
        std::fs::write(&plist, "plist").expect("write plist");

        let check = check_privileged_helper_artifacts_at(&helper, &plist, false);

        assert_eq!(check.status, DiagnosticStatus::Warning);
        assert!(check.detail.contains("PKG execution is disabled"));
        assert!(check.detail.contains("com.apm.pkg-helper"));
        assert!(check
            .hint
            .as_deref()
            .expect("hint")
            .contains("external PKG handoff"));
    }

    #[test]
    fn helper_artifacts_fail_when_execution_enabled_but_missing() {
        let env = tempfile::tempdir().expect("temp dir");
        let helper = env.path().join("com.apm.pkg-helper");
        std::fs::write(&helper, "helper").expect("write helper");

        let check =
            check_privileged_helper_artifacts_at(&helper, &env.path().join("missing.plist"), true);

        assert_eq!(check.status, DiagnosticStatus::Failure);
        assert!(check.detail.contains("execution is enabled"));
        assert!(check.detail.contains("1 artifact missing"));
    }
}
