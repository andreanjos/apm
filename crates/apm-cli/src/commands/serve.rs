use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
};

use anyhow::{bail, Context, Result};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use colored::Colorize;
use serde::{Deserialize, Serialize};

use apm_core::{
    config::Config,
    diagnostics::{run_diagnostics, DiagnosticsReport},
    engine::{
        ApmEngine, AvailableUpdatesRequest, InstallPlanRequest, InstalledPackageSort,
        InstalledPackagesRequest, PackageAccessFilter, PackageInstallStateFilter,
        PackageSearchRequest, SetPackagePinRequest,
    },
    registry::PluginFormat,
    service::{
        cache_model_catalog_manifest, cache_model_manifest, initialize_model_store,
        list_cached_models_matching, list_model_catalog, local_service_contract,
        loopback_token_file, model_store_layout, plan_cached_model_chain, plan_cached_model_run,
        remove_cached_model_package, service_health, validate_model_manifest,
        AutomaticResumePolicy, ExplicitRetryPolicy, LocalServiceContract, ModelCatalogListRequest,
        ModelCatalogListResult, ModelChainPlan, ModelChainPlanRequest, ModelListRequest,
        ModelListResult, ModelManifestCacheError, ModelManifestCacheRequest,
        ModelManifestCacheResult, ModelManifestValidationRequest, ModelManifestValidationResult,
        ModelRunPlan, ModelRunPlanRequest, PackagePinBody, PrivilegedInstallExecution,
        PrivilegedInstallPolicy, PrivilegedInstallPrerequisiteId,
        PrivilegedInstallPrerequisiteStatus, RuntimeBind, ServiceEndpointRuntime,
    },
};

mod auth;
mod model_operations;
mod operation_routes;
mod operations;

use auth::{require_loopback_token, LoopbackAuth};
use operation_routes::{
    cancel_operation, operation_events, operation_history, operation_recovery, operation_status,
    retry_operation, retry_recovery_operations, submit_archive_install, submit_library_scan,
    submit_model_install, submit_model_run, submit_model_weight_pull, submit_package_remove,
    submit_package_update, submit_registry_sync, submit_url_install,
};
use operations::OperationStore;

#[derive(Clone)]
struct ServeState {
    config: Config,
    engine: ApmEngine,
    bind: RuntimeBind,
    operations: OperationStore,
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    #[serde(default)]
    query: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    vendor: Option<String>,
    #[serde(default)]
    tag: Option<String>,
    #[serde(default)]
    access: PackageAccessFilter,
    #[serde(default)]
    install_state: PackageInstallStateFilter,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct DetailsQuery {
    #[serde(default)]
    include_versions: bool,
}

#[derive(Debug, Deserialize)]
struct LibraryQuery {
    #[serde(default)]
    format: Option<PluginFormat>,
    #[serde(default)]
    sort: InstalledPackageSort,
}

#[derive(Debug, Default, Deserialize)]
struct ModelQuery {
    #[serde(default)]
    query: String,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

struct ServiceHttpError {
    status: StatusCode,
    error: String,
}

type ServiceJson<T> = std::result::Result<Json<T>, ServiceHttpError>;
type ServiceAccepted<T> = std::result::Result<(StatusCode, Json<T>), ServiceHttpError>;

pub fn run_contract(json: bool) -> Result<()> {
    let contract = local_service_contract();
    if json {
        println!("{}", serde_json::to_string_pretty(&contract)?);
    } else {
        print_contract(&contract);
    }
    Ok(())
}

pub async fn run_server(config: Config, host: &str, port: Option<u16>, quiet: bool) -> Result<()> {
    let port = resolve_port(port)?;
    let ip = parse_loopback_host(host)?;
    let requested_addr = SocketAddr::new(ip, port);
    let listener = tokio::net::TcpListener::bind(requested_addr)
        .await
        .with_context(|| format!("Failed to bind apm service to {requested_addr}"))?;
    let local_addr = listener.local_addr()?;
    let bind = RuntimeBind {
        host: local_addr.ip().to_string(),
        port: local_addr.port(),
    };
    let router = serve_router(config, bind.clone())?;

    if !quiet {
        println!(
            "{} http://{}:{}",
            "apm service listening on".green().bold(),
            bind.host,
            bind.port
        );
        println!(
            "{} foreground preview; operation status is persisted under the service data dir",
            "mode:".dimmed()
        );
    }

    axum::serve(listener, router)
        .await
        .context("apm service stopped unexpectedly")
}

fn print_contract(contract: &LocalServiceContract) {
    println!("{}", "apm local service contract".bold());
    println!(
        "  {} {} ({})",
        "API:".dimmed(),
        contract.api_version,
        contract.schema_version
    );
    println!(
        "  {} {}:{}",
        "Bind:".dimmed(),
        contract.bind.host,
        contract.bind.default_port
    );
    println!(
        "  {} foreground preview; registry sync, library scan, handoff resolution, direct/archive install, update, pinning, removal, retry, and event streaming are available",
        "Status:".dimmed()
    );
    println!(
        "  {} localhost only; non-public routes require loopback token",
        "Security:".dimmed()
    );
    println!(
        "  {} {}; runs PKG installers: {}; {}",
        "Privileged installs:".dimmed(),
        privileged_install_execution_label(contract.security.privileged_install_policy.execution),
        contract
            .security
            .privileged_install_policy
            .runs_pkg_installers,
        privileged_install_prerequisite_summary(&contract.security.privileged_install_policy)
    );
    println!(
        "  {} {} at {}; receipts: {}",
        "Helper design:".dimmed(),
        contract
            .security
            .privileged_install_policy
            .design
            .helper
            .bundle_identifier,
        contract
            .security
            .privileged_install_policy
            .design
            .helper
            .install_path,
        contract
            .security
            .privileged_install_policy
            .design
            .rollback
            .receipt_store_relative_path
    );
    println!(
        "  {} {}; explicit retry: {}",
        "Operation recovery:".dimmed(),
        automatic_resume_label(contract.operation_recovery_policy.automatic_resume),
        explicit_retry_label(contract.operation_recovery_policy.explicit_retry)
    );
    println!(
        "  {} cancel: {}; retry: {}; progress: {}",
        "Operation controls:".dimmed(),
        contract.operation_control_policy.cancel_endpoint_id,
        contract.operation_control_policy.retry_endpoint_id,
        contract.operation_control_policy.progress_event_stream_id
    );

    println!();
    println!("{}", "Endpoints:".bold());
    for endpoint in &contract.endpoints {
        let request = endpoint.request.as_deref().unwrap_or("none");
        let runtime = match endpoint.runtime {
            ServiceEndpointRuntime::Available => "available".green(),
            ServiceEndpointRuntime::Planned => "planned".dimmed(),
        };
        println!(
            "  {:<4} {:<36} {:<9} {}",
            endpoint.method.to_string().cyan(),
            endpoint.path,
            runtime,
            endpoint.summary
        );
        println!(
            "       {} {}  {} {}",
            "request:".dimmed(),
            request,
            "response:".dimmed(),
            endpoint.response
        );
    }

    if !contract.event_streams.is_empty() {
        println!();
        println!("{}", "Event Streams:".bold());
        for stream in &contract.event_streams {
            let runtime = match stream.runtime {
                ServiceEndpointRuntime::Available => "available".green(),
                ServiceEndpointRuntime::Planned => "planned".dimmed(),
            };
            println!(
                "  {:<36} {:<9} {} ({})",
                stream.path,
                runtime,
                stream.events.join(", "),
                stream.transport
            );
        }
    }

    println!();
    println!("{}", "Pending Runtime Work:".bold());
    for item in &contract.pending_runtime_work {
        println!("  - {item}");
    }
}

fn serve_router(config: Config, bind: RuntimeBind) -> Result<Router> {
    let engine = ApmEngine::new(config.clone());
    let operations = OperationStore::new(operation_history_path(&config))?;
    let auth = LoopbackAuth::load_or_create(loopback_token_file(&config))?;
    let state = ServeState {
        config,
        engine,
        bind,
        operations,
    };

    let protected_routes = Router::new()
        .route("/v1/packages", get(search_packages))
        .route("/v1/diagnostics", get(diagnostics_report))
        .route("/v1/packages/{slug}", get(package_details))
        .route("/v1/library", get(installed_library))
        .route("/v1/library/updates", get(available_updates))
        .route("/v1/library/scan", post(submit_library_scan))
        .route("/v1/install/plan", post(install_plan))
        .route("/v1/models", get(model_packages))
        .route("/v1/models/catalog", get(model_catalog))
        .route(
            "/v1/models/catalog/{name}/{version}/cache",
            post(cache_model_catalog_manifest_body),
        )
        .route("/v1/models/store", get(model_store))
        .route("/v1/models/store/init", post(initialize_model_store_body))
        .route(
            "/v1/models/manifest/validate",
            post(validate_model_manifest_body),
        )
        .route("/v1/models/manifest/cache", post(cache_model_manifest_body))
        .route(
            "/v1/models/{name}/{version}/weights/pull",
            post(submit_model_weight_pull),
        )
        .route(
            "/v1/models/{name}/{version}/install",
            post(submit_model_install),
        )
        .route("/v1/models/{name}/{version}/run", post(submit_model_run))
        .route("/v1/models/{name}/{version}/run/plan", post(model_run_plan))
        .route("/v1/models/chains/plan", post(model_chain_plan))
        .route(
            "/v1/models/{name}/{version}",
            delete(remove_cached_model_body),
        )
        .route("/v1/registry/sync", post(submit_registry_sync))
        .route("/v1/install/handoff", post(install_handoff))
        .route("/v1/install/url", post(submit_url_install))
        .route("/v1/install/archive", post(submit_archive_install))
        .route("/v1/packages/{slug}/pin", post(set_package_pin))
        .route("/v1/packages/{slug}/update", post(submit_package_update))
        .route("/v1/packages/{slug}/remove", post(submit_package_remove))
        .route("/v1/operations", get(operation_history))
        .route("/v1/operations/recovery", get(operation_recovery))
        .route(
            "/v1/operations/recovery/retry",
            post(retry_recovery_operations),
        )
        .route("/v1/operations/{operation_id}", get(operation_status))
        .route(
            "/v1/operations/{operation_id}/cancel",
            post(cancel_operation),
        )
        .route("/v1/operations/{operation_id}/retry", post(retry_operation))
        .route(
            "/v1/operations/{operation_id}/events",
            get(operation_events),
        )
        .route_layer(middleware::from_fn_with_state(auth, require_loopback_token));

    Ok(Router::new()
        .route("/v1/health", get(health))
        .route("/v1/service/contract", get(contract))
        .merge(protected_routes)
        .with_state(state))
}

fn operation_history_path(config: &Config) -> PathBuf {
    config.resolved_data_dir().join("service/operations.json")
}

async fn health(State(state): State<ServeState>) -> Json<apm_core::service::ServiceHealth> {
    Json(service_health(&state.config, state.bind))
}

fn privileged_install_execution_label(execution: PrivilegedInstallExecution) -> &'static str {
    match execution {
        PrivilegedInstallExecution::ExternalHandoffOnly => "external handoff only",
    }
}

fn privileged_install_prerequisite_summary(policy: &PrivilegedInstallPolicy) -> String {
    let missing = policy
        .prerequisites
        .iter()
        .filter_map(|prerequisite| {
            (prerequisite.status == PrivilegedInstallPrerequisiteStatus::Missing)
                .then_some(privileged_install_prerequisite_label(prerequisite.id))
        })
        .collect::<Vec<_>>();
    let designed = policy
        .prerequisites
        .iter()
        .filter_map(|prerequisite| {
            (prerequisite.status == PrivilegedInstallPrerequisiteStatus::Designed)
                .then_some(privileged_install_prerequisite_label(prerequisite.id))
        })
        .collect::<Vec<_>>();

    if !missing.is_empty() {
        format!("missing gates: {}", missing.join(", "))
    } else if !designed.is_empty() {
        format!("designed gates: {}", designed.join(", "))
    } else {
        format!("{} privileged gates declared", policy.prerequisites.len())
    }
}

fn privileged_install_prerequisite_label(id: PrivilegedInstallPrerequisiteId) -> &'static str {
    match id {
        PrivilegedInstallPrerequisiteId::HelperOrEscalationDesign => "helper/escalation design",
        PrivilegedInstallPrerequisiteId::ExplicitUserConsent => "explicit user consent",
        PrivilegedInstallPrerequisiteId::PackageVerification => "package verification",
        PrivilegedInstallPrerequisiteId::AuditTrail => "audit trail",
        PrivilegedInstallPrerequisiteId::RollbackPlan => "rollback plan",
    }
}

async fn contract() -> Json<LocalServiceContract> {
    Json(local_service_contract())
}

async fn diagnostics_report(State(state): State<ServeState>) -> Json<DiagnosticsReport> {
    Json(run_diagnostics(&state.config))
}

async fn search_packages(
    State(state): State<ServeState>,
    Query(query): Query<SearchQuery>,
) -> ServiceJson<apm_core::engine::PackageSearchResult> {
    json_result(state.engine.search_packages(PackageSearchRequest {
        query: query.query,
        category: query.category,
        vendor: query.vendor,
        tag: query.tag,
        access: query.access,
        install_state: query.install_state,
        limit: query.limit,
    }))
}

async fn package_details(
    State(state): State<ServeState>,
    Path(slug): Path<String>,
    Query(query): Query<DetailsQuery>,
) -> ServiceJson<apm_core::engine::PackageDetailsResult> {
    json_result(state.engine.package_details(&slug, query.include_versions))
}

async fn installed_library(
    State(state): State<ServeState>,
    Query(query): Query<LibraryQuery>,
) -> ServiceJson<Vec<apm_core::engine::InstalledPackageSummary>> {
    json_result(state.engine.installed_packages(InstalledPackagesRequest {
        format: query.format,
        sort: query.sort,
    }))
}

async fn available_updates(
    State(state): State<ServeState>,
) -> ServiceJson<apm_core::engine::AvailableUpdatesResult> {
    json_result(state.engine.available_updates(AvailableUpdatesRequest))
}

async fn install_plan(
    State(state): State<ServeState>,
    Json(request): Json<InstallPlanRequest>,
) -> ServiceJson<apm_core::engine::InstallPlanResult> {
    json_result(state.engine.plan_install(request))
}

async fn install_handoff(
    State(state): State<ServeState>,
    Json(request): Json<InstallPlanRequest>,
) -> ServiceJson<apm_core::engine::InstallHandoffResult> {
    json_result(state.engine.install_handoff(request))
}

async fn model_store() -> Json<apm_core::service::ModelStoreLayout> {
    Json(model_store_layout())
}

async fn initialize_model_store_body() -> ServiceJson<apm_core::service::ModelStoreInitResult> {
    json_result(initialize_model_store())
}

async fn model_packages(Query(query): Query<ModelQuery>) -> ServiceJson<ModelListResult> {
    json_result(list_cached_models_matching(ModelListRequest {
        query: query.query,
    }))
}

async fn model_catalog(
    State(state): State<ServeState>,
    Query(query): Query<ModelQuery>,
) -> ServiceJson<ModelCatalogListResult> {
    json_result(list_model_catalog(
        &state.config,
        ModelCatalogListRequest { query: query.query },
    ))
}

async fn cache_model_catalog_manifest_body(
    State(state): State<ServeState>,
    Path((name, version)): Path<(String, String)>,
) -> ServiceJson<ModelManifestCacheResult> {
    cache_model_catalog_manifest(&state.config, name, version)
        .map(Json)
        .map_err(|error| match error {
            ModelManifestCacheError::Invalid(message) => ServiceHttpError::bad_request(message),
            ModelManifestCacheError::Store(error) => ServiceHttpError::internal(error),
        })
}

async fn validate_model_manifest_body(
    Json(request): Json<ModelManifestValidationRequest>,
) -> ServiceJson<ModelManifestValidationResult> {
    validate_model_manifest(request)
        .map(Json)
        .map_err(|error| ServiceHttpError::bad_request(error.to_string()))
}

async fn cache_model_manifest_body(
    Json(request): Json<ModelManifestCacheRequest>,
) -> ServiceJson<ModelManifestCacheResult> {
    cache_model_manifest(request)
        .map(Json)
        .map_err(|error| match error {
            ModelManifestCacheError::Invalid(message) => ServiceHttpError::bad_request(message),
            ModelManifestCacheError::Store(error) => ServiceHttpError::internal(error),
        })
}

async fn remove_cached_model_body(
    Path((name, version)): Path<(String, String)>,
) -> ServiceJson<apm_core::model::ModelRemoveResult> {
    json_result(remove_cached_model_package(name, version))
}

async fn set_package_pin(
    State(state): State<ServeState>,
    Path(slug): Path<String>,
    Json(request): Json<PackagePinBody>,
) -> ServiceJson<apm_core::engine::SetPackagePinResult> {
    json_result(state.engine.set_package_pin(SetPackagePinRequest {
        slug,
        pinned: request.pinned,
    }))
}

async fn model_run_plan(
    Path((name, version)): Path<(String, String)>,
    Json(request): Json<ModelRunPlanRequest>,
) -> ServiceJson<ModelRunPlan> {
    plan_cached_model_run(name, version, request)
        .map(Json)
        .map_err(|error| ServiceHttpError::bad_request(error.to_string()))
}

async fn model_chain_plan(
    Json(request): Json<ModelChainPlanRequest>,
) -> ServiceJson<ModelChainPlan> {
    plan_cached_model_chain(request)
        .map(Json)
        .map_err(|error| ServiceHttpError::bad_request(error.to_string()))
}

fn automatic_resume_label(policy: AutomaticResumePolicy) -> &'static str {
    match policy {
        AutomaticResumePolicy::Disabled => "automatic resume disabled",
    }
}

fn explicit_retry_label(policy: ExplicitRetryPolicy) -> &'static str {
    match policy {
        ExplicitRetryPolicy::RequestMetadataRequired => "request metadata required",
    }
}

fn json_result<T>(result: Result<T>) -> ServiceJson<T> {
    result.map(Json).map_err(ServiceHttpError::internal)
}

impl ServiceHttpError {
    fn internal(error: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: error.to_string(),
        }
    }

    fn not_found(error: String) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            error,
        }
    }

    fn bad_request(error: String) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            error,
        }
    }

    fn conflict(error: String) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            error,
        }
    }

    fn unauthorized(error: String) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            error,
        }
    }
}

impl IntoResponse for ServiceHttpError {
    fn into_response(self) -> Response {
        (self.status, Json(ErrorResponse { error: self.error })).into_response()
    }
}

fn resolve_port(port: Option<u16>) -> Result<u16> {
    if let Some(port) = port {
        return Ok(port);
    }

    let contract = local_service_contract();
    let port_env = contract.bind.port_env;
    match std::env::var(&port_env) {
        Ok(value) => value
            .parse::<u16>()
            .with_context(|| format!("{port_env} must be a TCP port number")),
        Err(std::env::VarError::NotPresent) => Ok(contract.bind.default_port),
        Err(error) => Err(error).context(format!("Cannot read {port_env}")),
    }
}

fn parse_loopback_host(host: &str) -> Result<IpAddr> {
    let ip = if host == "localhost" {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    } else {
        host.parse::<IpAddr>()
            .with_context(|| format!("Invalid host '{host}'"))?
    };

    if !ip.is_loopback() {
        bail!("apm serve only binds to loopback hosts; got {host}");
    }

    Ok(ip)
}
