// env command — print apm-relevant environment info for bug reports.

use anyhow::Result;
use serde::Serialize;

use apm_core::config;

use crate::utils::display_path;

// ── JSON output type ─────────────────────────────────────────────────────────

#[derive(Serialize)]
struct EnvInfo {
    apm_version: String,
    os: String,
    arch: String,
    config_dir: String,
    data_dir: String,
    cache_dir: String,
    au_plugin_dir: String,
    vst3_plugin_dir: String,
    system_au_plugin_dir: String,
    system_vst3_plugin_dir: String,
}

// ── Public entry point ───────────────────────────────────────────────────────

pub fn run(json: bool) -> Result<()> {
    let version = env!("CARGO_PKG_VERSION").to_string();
    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();

    let cfg_dir = config::config_dir();
    let dat_dir = config::data_dir();
    let cch_dir = config::cache_dir();
    let au_dir = config::user_au_dir();
    let vst3_dir = config::user_vst3_dir();
    let sys_au_dir = config::system_au_dir();
    let sys_vst3_dir = config::system_vst3_dir();

    if json {
        let info = EnvInfo {
            apm_version: version,
            os,
            arch,
            config_dir: display_path(&cfg_dir),
            data_dir: display_path(&dat_dir),
            cache_dir: display_path(&cch_dir),
            au_plugin_dir: display_path(&au_dir),
            vst3_plugin_dir: display_path(&vst3_dir),
            system_au_plugin_dir: display_path(&sys_au_dir),
            system_vst3_plugin_dir: display_path(&sys_vst3_dir),
        };
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        println!("apm environment:");
        println!("  apm version:          {version}");
        println!("  OS:                   {os} {arch}");
        println!("  Config dir:           {}", display_path(&cfg_dir));
        println!("  Data dir:             {}", display_path(&dat_dir));
        println!("  Cache dir:            {}", display_path(&cch_dir));
        println!("  AU plugin dir:        {}", display_path(&au_dir));
        println!("  VST3 plugin dir:      {}", display_path(&vst3_dir));
        println!("  System AU dir:        {}", display_path(&sys_au_dir));
        println!("  System VST3 dir:      {}", display_path(&sys_vst3_dir));
    }

    Ok(())
}
