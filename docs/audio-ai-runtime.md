# apm Audio-AI Runtime

`apm` is an audio package manager with plugins as its first useful package type:
a single CLI and local store for installable audio software plus open-source
audio-AI model packages.

The audio-AI track is intentionally native-first and manifest-driven. A model
package declares its runtime mode, weights, typed IO, parameters, license, and
hardware requirements in TOML. The CLI can validate that manifest and generate a
reproducible `apm.lock` before any runtime adapter exists.

## Initial Boundary

The current Rust CLI already manages AU/VST3 plugin installation. The model
runtime work starts under an explicit `apm model` command group so existing
plugin commands remain stable while the new package/runtime layer takes shape.

Implemented first:

- model package manifest schema
- `native-mlx`, `coreml`, and `python-env` runtime mode declarations
- typed IO ports: `audio`, `stems`, `midi`, `text`, `embedding`, `spectrogram`
- parameter declarations for generated UI and chain validation
- `apm.lock` generation from exact manifest metadata and weight hashes
- `~/.apm` model-store layout: manifests, weights, runtimes, cache, logs
- desktop Audio-AI store layout rendering from the authenticated service
  snapshot, plus setup-panel initialization through the same store initializer
  used by `apm model store --init`
- curated registry model catalog discovery from configured registry sources
- `apm serve` endpoints for cached model package listing/search, model store
  layout and initialization, content-based manifest validation, content-based
  manifest caching, curated model catalog search, curated registry manifest
  import, cached model removal, and cached model installation
- `apm model list`, `apm model search`, `apm model pull`,
  `apm model install`, `apm model rm`, and authenticated service model-pull
  and model-install operations for verified, content-addressed weight caching
- adapter provisioning traits for `native-mlx`, `coreml`, and `python-env`
  that prepare runtime metadata under `~/.apm/runtimes/<mode>/<name>/<version>`
- `POST /v1/models/{name}/{version}/run/plan` for non-mutating run plans that
  bind cached manifests, verified weights, typed `adapter.toml` metadata, and
  manifest-validated parameter values without executing model code. Planning
  now rejects stale adapter metadata whose package, mode, entry, adapter, hash,
  or weight path no longer matches the cached manifest and local store, and the
  typed execution readiness currently blocks on `adapter_runner_unavailable`
- `apm model run ... --input ... --output ...` now uses the same core runner
  boundary as the service operation path and returns a structured blocked
  `model_run` result until adapter runners exist
- `POST /v1/models/{name}/{version}/run` as an authenticated, request-backed
  operation lane that validates the same plan, emits `model_run_blocked` for
  today's adapter-unavailable path, and already has a `model_run_completed`
  event/result lane for the first executable adapter runner. Completed results
  are accepted only for plans whose typed execution readiness is `ready`; a
  runner cannot report success for a plan that still carries
  `adapter_runner_unavailable`
- a core `ModelRunner` execution boundary whose current default runner returns
  the structured `adapter_runner_unavailable` blocker instead of scattering
  runner-unavailable special cases through service or desktop code
- `POST /v1/models/chains/plan` for non-mutating chain plans that validate
  each step's prepared adapter metadata, typed IO edges, and the explicit
  `stems -> audio` stem-selection handoff without executing model code
- desktop Audio-AI run-plan action plus a service-backed execution-readiness
  check, both choosing an input audio file and output folder before rendering
  the non-mutating plan or structured blocked run result
- model pull/install operation events in service history, SSE progress streams,
  and the desktop Audio-AI panel, plus model-run started/blocked/failed events
  in desktop progress and operation history for the future runner lane
- model-run cancellation checkpoints before planning and before returning the
  current structured blocked result

Next runtime milestones:

1. Replace the current unavailable `ModelRunner` implementation with executable
   native MLX/Core ML adapters and a managed Python environment fallback.
2. Implement one native Apple Silicon package and one managed Python fallback.
3. Extend `apm serve` from run/chain planning into DAW-safe runtime session
   endpoints that reuse the current model-run operation progress and
   cancellation contract.

## Early Model Scope

The early registry should favor utility models where local Mac execution has a
clear user advantage: privacy, offline use, repeatability, and avoiding hosted
per-minute cost.

Initial curated categories:

- Stem separation, anchored by Demucs/HTDemucs and one Roformer-style separator
  to prove multiple packages in the same workflow family.
- Transcription, anchored by Whisper-class audio-to-text models.
- Audio-to-MIDI, restoration, or mastering utilities as the first managed
  `python-env` fallback proof.

Deferred or excluded categories:

- Generative music models are a bonus tier, not the headline. They should wait
  until the utility-model runtime path is honest and repeatable.
- Raw artist voice-clone weights are excluded from the curated v0 registry.
  Voice conversion tools can be packaged, but pre-trained packages that clone a
  real identifiable artist should not be first-party curated content.
- Full DAW/plugin clients remain a follow-on surface after the local service and
  runtime sessions are stable.

## Commands

```sh
apm model validate examples/models/demucs.toml
apm model info examples/models/demucs.toml
apm model lock examples/models/demucs.toml -o apm.lock
apm model store --init
apm model list
apm model search stems
apm model search --available stems
apm model pull path/to/model-with-direct-weights.toml
apm model pull name@version
apm model install demucs@4.0.1
apm model run demucs@4.0.1 --input mix.wav --output stems/ --param stems=4
apm model rm demucs@4.0.1
```

Weight pull currently supports direct `http(s)` sources and explicit Hugging
Face file sources such as `hf:org/repo/model.safetensors` or
`hf:org/repo@revision/model.safetensors`. Repo-only `hf:` sources are rejected
until registry metadata can identify the exact file to download.

These commands are scaffolding, not the final UX. The target user-facing model
surface remains the smaller verb set from the product brief: `search`, `pull`,
`run`, `list`, `info`, `lock`, `install`, `serve`, and `rm`.
