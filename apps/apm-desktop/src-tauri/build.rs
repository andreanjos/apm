fn main() {
    const CHANNEL_ENV: &str = "APM_DESKTOP_DISTRIBUTION_CHANNEL";

    println!("cargo:rerun-if-env-changed={CHANNEL_ENV}");
    let channel = std::env::var(CHANNEL_ENV).unwrap_or_else(|_| "preview".to_string());
    println!("cargo:rustc-env=APM_DESKTOP_DISTRIBUTION_CHANNEL={channel}");
    tauri_build::build()
}
