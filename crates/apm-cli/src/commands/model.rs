use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use colored::Colorize;
use serde::Serialize;

use apm_core::{
    config,
    model::{
        install_cached_model, model_manifest_matches_query, pull_model_weights,
        remove_cached_model, run_model as execute_model_run, ModelCatalog, ModelCatalogPackage,
        ModelInstallResult, ModelLockfile, ModelManifest, ModelRemoveStatus, ModelRunParamBinding,
        ModelRunParamValue, ModelRunPlan, ModelRunPlanRequest, ModelRunResult, ModelRunStatus,
        ModelStore,
    },
};

#[derive(Subcommand, Debug)]
pub enum ModelCommands {
    /// Validate an audio-AI package manifest.
    Validate {
        /// Path to a model package manifest TOML file.
        manifest: PathBuf,
    },

    /// Show audio-AI package manifest details.
    Info {
        /// Path to a model package manifest TOML file.
        manifest: PathBuf,
    },

    /// Write an apm.lock file from one or more model manifests.
    Lock {
        /// Model package manifest TOML files to lock.
        #[arg(required = true)]
        manifests: Vec<PathBuf>,

        /// Output lockfile path.
        #[arg(long, short = 'o', default_value = "apm.lock")]
        output: PathBuf,
    },

    /// Print the local audio-AI model store layout.
    Store {
        /// Create the store directories if they are missing.
        #[arg(long)]
        init: bool,
    },

    /// List cached audio-AI model package manifests.
    List,

    /// Search cached audio-AI model package manifests.
    Search {
        /// Search configured model registries instead of the local model store.
        #[arg(long)]
        available: bool,

        /// Search terms matched against cached model metadata.
        #[arg(required = true)]
        query: Vec<String>,
    },

    /// Pull and verify a model package's weights into the local store.
    Pull {
        /// Path to a model package manifest TOML file.
        manifest: PathBuf,
    },

    /// Install a cached audio-AI model package into the local store.
    Install {
        /// Cached model package ID, such as demucs@4.0.1.
        package: String,
    },

    /// Attempt a run from prepared runtime metadata.
    Run {
        /// Cached model package ID, such as demucs@4.0.1.
        package: String,

        /// Input audio path for the future runtime invocation.
        #[arg(long)]
        input: PathBuf,

        /// Output path for the future runtime result.
        #[arg(long)]
        output: PathBuf,

        /// Runtime parameter binding, repeated as KEY=VALUE.
        #[arg(long = "param", value_name = "KEY=VALUE")]
        params: Vec<String>,
    },

    /// Remove a cached audio-AI model package manifest and unreferenced weights.
    Rm {
        /// Cached model package ID, such as demucs@4.0.1.
        package: String,
    },
}

#[derive(Serialize)]
struct ValidationJson<'a> {
    valid: bool,
    package: &'a str,
    version: &'a str,
    mode: String,
    input: String,
    output: String,
}

#[derive(Serialize)]
struct LockJson {
    output: String,
    packages: usize,
}

#[derive(Serialize)]
struct StoreJson {
    root: String,
    manifests: String,
    weights: String,
    runtimes: String,
    cache: String,
    logs: String,
    config: String,
}

#[derive(Serialize)]
struct ListJson {
    packages: Vec<ListPackageJson>,
}

#[derive(Serialize)]
struct ListPackageJson {
    package: String,
    name: String,
    version: String,
    runtime: String,
    input: String,
    output: String,
    weights_cached: bool,
    weights_sha256: String,
}

#[derive(Serialize)]
struct PullJson {
    package: String,
    status: String,
    source: String,
    resolved_url: String,
    sha256: String,
    path: String,
    bytes: u64,
    manifest_path: String,
}

#[derive(Serialize)]
struct RemoveJson {
    package: String,
    status: String,
    manifest_path: String,
    runtime_dir: Option<String>,
    weight_path: Option<String>,
    removed_manifest: bool,
    removed_runtime: bool,
    removed_weight: bool,
    weight_still_referenced: bool,
}

pub async fn run(command: &ModelCommands, json: bool) -> Result<()> {
    match command {
        ModelCommands::Validate { manifest } => validate(manifest, json),
        ModelCommands::Info { manifest } => info(manifest, json),
        ModelCommands::Lock { manifests, output } => lock(manifests, output, json),
        ModelCommands::Store { init } => store(*init, json),
        ModelCommands::List => list(None, json),
        ModelCommands::Search { available, query } => {
            if *available {
                search_available(query.join(" "), json)
            } else {
                list(Some(query.join(" ")), json)
            }
        }
        ModelCommands::Pull { manifest } => {
            let manifest = manifest.clone();
            tokio::task::spawn_blocking(move || pull(&manifest, json))
                .await
                .context("model pull task failed")?
        }
        ModelCommands::Install { package } => {
            let package = package.clone();
            tokio::task::spawn_blocking(move || install(&package, json))
                .await
                .context("model install task failed")?
        }
        ModelCommands::Run {
            package,
            input,
            output,
            params,
        } => run_prepared_model(package, input, output, params, json),
        ModelCommands::Rm { package } => remove(package, json),
    }
}

fn validate(path: &Path, json: bool) -> Result<()> {
    let manifest = ModelManifest::from_path(path)?;
    if json {
        let result = ValidationJson {
            valid: true,
            package: &manifest.package.name,
            version: &manifest.package.version,
            mode: manifest.runtime.mode.to_string(),
            input: manifest.io.input.to_string(),
            output: manifest.io.output.to_string(),
        };
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!(
            "{} {} {}",
            "valid".green().bold(),
            manifest.package_id().bold(),
            format!(
                "({}: {} -> {})",
                manifest.runtime.mode, manifest.io.input, manifest.io.output
            )
            .dimmed()
        );
    }
    Ok(())
}

fn info(path: &Path, json: bool) -> Result<()> {
    let manifest = ModelManifest::from_path(path)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&manifest)?);
        return Ok(());
    }

    println!("{}", manifest.package_id().bold());
    println!("{}", "\u{2500}".repeat(47).dimmed());
    println!(
        "{:<12} {}",
        "Description:".dimmed(),
        manifest.package.description
    );
    println!(
        "{:<12} {}",
        "Publisher:".dimmed(),
        manifest.package.publisher
    );
    println!("{:<12} {}", "Runtime:".dimmed(), manifest.runtime.mode);
    println!("{:<12} {}", "Entry:".dimmed(), manifest.runtime.entry);
    println!(
        "{:<12} {} -> {}",
        "IO:".dimmed(),
        manifest.io.input,
        manifest.io.output
    );
    println!("{:<12} {}", "Weights:".dimmed(), manifest.weights.source);
    println!("{:<12} {}", "Format:".dimmed(), manifest.weights.format);
    println!("{:<12} {}", "SHA256:".dimmed(), manifest.weights.sha256);
    println!(
        "{:<12} {} / commercial: {}",
        "License:".dimmed(),
        manifest.license.spdx,
        manifest.license.commercial
    );
    println!(
        "{:<12} {} GB",
        "Memory:".dimmed(),
        manifest.hardware.min_memory_gb
    );
    if !manifest.hardware.requires.is_empty() {
        println!(
            "{:<12} {}",
            "Requires:".dimmed(),
            manifest.hardware.requires.join(", ")
        );
    }
    if !manifest.params.is_empty() {
        println!();
        println!("{}", "Parameters:".bold());
        for param in &manifest.params {
            println!("  {:<14} {}", param.name.bold(), param.param_type);
        }
    }
    Ok(())
}

fn lock(paths: &[PathBuf], output: &Path, json: bool) -> Result<()> {
    let manifests: Vec<ModelManifest> = paths
        .iter()
        .map(|path| ModelManifest::from_path(path))
        .collect::<Result<Vec<_>>>()?;
    let lockfile = ModelLockfile::from_manifests(&manifests)?;
    lockfile.write_to_path(output)?;

    if json {
        let result = LockJson {
            output: output.display().to_string(),
            packages: lockfile.packages.len(),
        };
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!(
            "{} {} {}",
            "wrote".green().bold(),
            output.display(),
            format!(
                "({} package{})",
                lockfile.packages.len(),
                if lockfile.packages.len() == 1 {
                    ""
                } else {
                    "s"
                }
            )
            .dimmed()
        );
    }
    Ok(())
}

fn store(init: bool, json: bool) -> Result<()> {
    let store = ModelStore::default();
    if init {
        store.ensure()?;
    }

    let result = StoreJson {
        root: store.root().display().to_string(),
        manifests: store.manifests_dir().display().to_string(),
        weights: store.weights_dir().display().to_string(),
        runtimes: store.runtimes_dir().display().to_string(),
        cache: store.cache_dir().display().to_string(),
        logs: store.logs_dir().display().to_string(),
        config: store.config_file().display().to_string(),
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        if init {
            println!("{}", "initialized model store".green().bold());
        }
        println!("{:<11} {}", "Root:".dimmed(), result.root);
        println!("{:<11} {}", "Manifests:".dimmed(), result.manifests);
        println!("{:<11} {}", "Weights:".dimmed(), result.weights);
        println!("{:<11} {}", "Runtimes:".dimmed(), result.runtimes);
        println!("{:<11} {}", "Cache:".dimmed(), result.cache);
        println!("{:<11} {}", "Logs:".dimmed(), result.logs);
        println!("{:<11} {}", "Config:".dimmed(), result.config);
    }
    Ok(())
}

fn list(query: Option<String>, json: bool) -> Result<()> {
    let store = ModelStore::default();
    let mut packages = store.cached_manifests()?;
    if let Some(query) = query.as_deref() {
        packages.retain(|manifest| model_manifest_matches_query(manifest, query));
    }
    packages.sort_by(|left, right| {
        left.package
            .name
            .cmp(&right.package.name)
            .then_with(|| left.package.version.cmp(&right.package.version))
    });

    let rows = packages
        .iter()
        .map(|manifest| ListPackageJson {
            package: manifest.package_id(),
            name: manifest.package.name.clone(),
            version: manifest.package.version.clone(),
            runtime: manifest.runtime.mode.to_string(),
            input: manifest.io.input.to_string(),
            output: manifest.io.output.to_string(),
            weights_cached: store.weight_path(&manifest.weights.sha256).exists(),
            weights_sha256: manifest.weights.sha256.clone(),
        })
        .collect::<Vec<_>>();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ListJson { packages: rows })?
        );
        return Ok(());
    }

    if rows.is_empty() {
        let message = if query.is_some() {
            "no cached model packages matched"
        } else {
            "no model packages cached"
        };
        println!("{}", message.dimmed());
        return Ok(());
    }

    for row in rows {
        let weight_state = if row.weights_cached {
            "weights cached".green()
        } else {
            "weights missing".yellow()
        };
        println!(
            "{} {} {} {}",
            row.package.bold(),
            format!("({})", row.runtime).dimmed(),
            format!("{} -> {}", row.input, row.output).dimmed(),
            weight_state
        );
    }
    Ok(())
}

fn search_available(query: String, json: bool) -> Result<()> {
    let config = config::init()?;
    let catalog = ModelCatalog::load_all_sources(&config)?;
    let store = ModelStore::default();
    let rows = catalog
        .search(&query)
        .into_iter()
        .map(|package| catalog_package_row(&store, package))
        .collect::<Vec<_>>();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ListJson { packages: rows })?
        );
        return Ok(());
    }

    if rows.is_empty() {
        println!("{}", "no registry model packages matched".dimmed());
        return Ok(());
    }

    for row in rows {
        let weight_state = if row.weights_cached {
            "weights cached".green()
        } else {
            "weights missing".yellow()
        };
        println!(
            "{} {} {} {}",
            row.package.bold(),
            format!("({})", row.runtime).dimmed(),
            format!("{} -> {}", row.input, row.output).dimmed(),
            weight_state
        );
    }
    Ok(())
}

fn catalog_package_row(store: &ModelStore, package: &ModelCatalogPackage) -> ListPackageJson {
    let manifest = &package.manifest;
    ListPackageJson {
        package: manifest.package_id(),
        name: manifest.package.name.clone(),
        version: manifest.package.version.clone(),
        runtime: manifest.runtime.mode.to_string(),
        input: manifest.io.input.to_string(),
        output: manifest.io.output.to_string(),
        weights_cached: store.weight_path(&manifest.weights.sha256).exists(),
        weights_sha256: manifest.weights.sha256.clone(),
    }
}

fn pull(path: &Path, json: bool) -> Result<()> {
    let raw = resolve_model_manifest_input(path)?;
    let manifest = ModelManifest::from_toml_str(&raw)?;
    let store = ModelStore::default();
    let result = pull_model_weights(&store, &manifest)?;
    let cached = store.cache_manifest(&manifest, &raw)?;
    let status = format!("{:?}", result.status).to_lowercase();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&PullJson {
                package: result.package_id,
                status,
                source: result.source,
                resolved_url: result.resolved_url,
                sha256: result.sha256,
                path: result.path,
                bytes: result.bytes,
                manifest_path: cached.path.display().to_string(),
            })?
        );
    } else {
        println!(
            "{} {} {}",
            status.green().bold(),
            manifest.package_id().bold(),
            format!("({} bytes)", result.bytes).dimmed()
        );
        println!("{:<12} {}", "Weights:".dimmed(), result.path);
        println!("{:<12} {}", "Manifest:".dimmed(), cached.path.display());
    }

    Ok(())
}

fn resolve_model_manifest_input(input: &Path) -> Result<String> {
    if input.exists() {
        return fs::read_to_string(input)
            .with_context(|| format!("Cannot read model manifest: {}", input.display()));
    }

    let package_id = input.to_string_lossy();
    let (name, version) = parse_optional_package_id(&package_id)?;
    let config = config::init()?;
    let catalog = ModelCatalog::load_all_sources(&config)?;
    let package = catalog.find(name, version).with_context(|| {
        format!("model package not found in configured registries: {package_id}")
    })?;
    Ok(package.manifest_toml.clone())
}

fn install(package: &str, json: bool) -> Result<()> {
    let (name, version) = parse_package_id(package)?;
    let store = ModelStore::default();
    let result = install_cached_model(&store, name, version)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        print_install_result(&result);
    }

    Ok(())
}

fn print_install_result(result: &ModelInstallResult) {
    println!(
        "{} {} {}",
        "ready".green().bold(),
        result.package_id.bold(),
        format!("({})", result.runtime_mode).dimmed()
    );
    println!("{:<12} {}", "Entry:".dimmed(), result.runtime_entry);
    println!("{:<12} {}", "Manifest:".dimmed(), result.manifest_path);
    println!(
        "{:<12} {} {}",
        "Adapter:".dimmed(),
        result.runtime.adapter,
        format!("({})", result.runtime.status).dimmed()
    );
    println!(
        "{:<12} {}",
        "Runtime dir:".dimmed(),
        result.runtime.runtime_dir
    );
    println!("{:<12} {}", "Weights:".dimmed(), result.weights.path);
    println!(
        "{:<12} {}",
        "Weight IO:".dimmed(),
        format!(
            "{} / {} bytes",
            format!("{:?}", result.weights.status).to_lowercase(),
            result.weights.bytes
        )
        .dimmed()
    );
}

fn run_prepared_model(
    package: &str,
    input: &Path,
    output: &Path,
    params: &[String],
    json: bool,
) -> Result<()> {
    let (name, version) = parse_package_id(package)?;
    let store = ModelStore::default();
    let params = parse_run_params(params)?;
    let result = execute_model_run(
        &store,
        name,
        version,
        ModelRunPlanRequest {
            input_path: input.display().to_string(),
            output_path: output.display().to_string(),
            params,
        },
    )?;

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        print_run_result(&result);
    }

    Ok(())
}

fn print_run_result(result: &ModelRunResult) {
    let status = match result.status() {
        ModelRunStatus::Completed => "completed".green().bold(),
        ModelRunStatus::Blocked => "blocked".yellow().bold(),
    };
    println!(
        "{} {} {}",
        status,
        result.package_id().bold(),
        format!("({})", result.plan().runtime_mode).dimmed()
    );
    print_run_details(result.plan());
    println!("{:<12} {}", "Result:".dimmed(), result.message());
}

fn print_run_details(plan: &ModelRunPlan) {
    println!("{:<12} {}", "Adapter:".dimmed(), plan.adapter);
    println!("{:<12} {}", "Entry:".dimmed(), plan.runtime_entry);
    println!("{:<12} {}", "Input:".dimmed(), plan.input_path);
    println!("{:<12} {}", "Output:".dimmed(), plan.output_path);
    if !plan.params.is_empty() {
        println!(
            "{:<12} {}",
            "Params:".dimmed(),
            run_param_list(&plan.params)
        );
    }
    println!("{:<12} {}", "Runtime:".dimmed(), plan.runtime_dir);
    println!(
        "{:<12} {}",
        "Adapter TOML:".dimmed(),
        plan.adapter_manifest_path
    );
    println!("{:<12} {}", "Weights:".dimmed(), plan.weights_path);
    println!("{:<12} {}", "Execution:".dimmed(), plan.execution.message());
    println!("{:<12} {}", "Plan:".dimmed(), plan.message);
}

fn parse_run_params(params: &[String]) -> Result<BTreeMap<String, ModelRunParamValue>> {
    let mut parsed = BTreeMap::new();
    for raw in params {
        let (name, value) = raw
            .split_once('=')
            .with_context(|| format!("model run parameter must be KEY=VALUE: {raw}"))?;
        if name.trim().is_empty() {
            bail!("model run parameter name must not be empty");
        }
        if parsed
            .insert(
                name.to_string(),
                ModelRunParamValue::String(value.to_string()),
            )
            .is_some()
        {
            bail!("duplicate model run parameter '{name}'");
        }
    }
    Ok(parsed)
}

fn run_param_list(params: &[ModelRunParamBinding]) -> String {
    params
        .iter()
        .map(|param| format!("{}={}", param.name, run_param_value(&param.value)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn run_param_value(value: &ModelRunParamValue) -> String {
    match value {
        ModelRunParamValue::String(value) => value.clone(),
        ModelRunParamValue::Integer(value) => value.to_string(),
        ModelRunParamValue::Float(value) => value.to_string(),
        ModelRunParamValue::Boolean(value) => value.to_string(),
    }
}

fn remove(package: &str, json: bool) -> Result<()> {
    let (name, version) = parse_package_id(package)?;
    let store = ModelStore::default();
    let result = remove_cached_model(&store, name, version)?;
    let status = match result.status {
        ModelRemoveStatus::Removed => "removed",
        ModelRemoveStatus::NotCached => "not_cached",
    };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&RemoveJson {
                package: result.package_id,
                status: status.to_string(),
                manifest_path: result.manifest_path,
                runtime_dir: result.runtime_dir,
                weight_path: result.weight_path,
                removed_manifest: result.removed_manifest,
                removed_runtime: result.removed_runtime,
                removed_weight: result.removed_weight,
                weight_still_referenced: result.weight_still_referenced,
            })?
        );
        return Ok(());
    }

    match result.status {
        ModelRemoveStatus::Removed => {
            println!("{} {}", "removed".green().bold(), result.package_id.bold());
            println!("{:<12} {}", "Manifest:".dimmed(), result.manifest_path);
            if let Some(weight_path) = result.weight_path {
                let action = if result.removed_weight {
                    "removed"
                } else if result.weight_still_referenced {
                    "kept; still referenced"
                } else {
                    "not present"
                };
                println!(
                    "{:<12} {} {}",
                    "Weights:".dimmed(),
                    weight_path,
                    action.dimmed()
                );
            }
        }
        ModelRemoveStatus::NotCached => {
            println!("{} {}", "not cached".yellow().bold(), result.package_id);
        }
    }

    Ok(())
}

fn parse_package_id(package: &str) -> Result<(&str, &str)> {
    let (name, version) = package
        .split_once('@')
        .ok_or_else(|| anyhow::anyhow!("model package must be name@version, got {package}"))?;
    if name.is_empty() || version.is_empty() || version.contains('@') {
        bail!("model package must be name@version, got {package}");
    }
    Ok((name, version))
}

fn parse_optional_package_id(package: &str) -> Result<(&str, Option<&str>)> {
    if let Some((name, version)) = package.split_once('@') {
        if name.is_empty() || version.is_empty() || version.contains('@') {
            bail!("model package must be name or name@version, got {package}");
        }
        return Ok((name, Some(version)));
    }
    if package.is_empty() {
        bail!("model package must be name or name@version, got {package}");
    }
    Ok((package, None))
}
