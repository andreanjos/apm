#[test]
fn discovery_stub_regressions_removed() {
    let crate_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let featured = std::fs::read_to_string(crate_root.join("src/commands/featured.rs")).unwrap();
    let explore = std::fs::read_to_string(crate_root.join("src/commands/explore.rs")).unwrap();

    assert!(!featured.contains("COMMERCE_NOT_AVAILABLE"));
    assert!(!explore.contains("COMMERCE_NOT_AVAILABLE"));
}
