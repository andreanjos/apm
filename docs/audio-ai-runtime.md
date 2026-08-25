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
- `~/.apm` model-store layout: manifests, weights, runtimes, cache, and logs
- curated registry model catalog discovery from configured registry sources
- `apm model list`, `search`, `pull`, `install`, and `rm` for verified,
  content-addressed model packages
- adapter provisioning traits for `native-mlx`, `coreml`, and `python-env`
  that prepare runtime metadata under `~/.apm/runtimes/<mode>/<name>/<version>`
- non-mutating run plans that bind cached manifests, verified weights, typed
  `adapter.toml` metadata, and manifest-validated parameters before execution
- stale-adapter rejection when package, mode, entry, adapter, hash, or weight
  path metadata no longer matches the cached manifest and local store
- `apm model run ... --input ... --output ...`, routed through the core
  `ModelRunner` boundary and returning the structured
  `adapter_runner_unavailable` blocker until executable adapters exist
- model-run cancellation checkpoints before planning and before returning the
  current blocked result

Next runtime milestones:

1. Replace the current unavailable `ModelRunner` implementation with executable
   native MLX/Core ML adapters and a managed Python environment fallback.
2. Implement one native Apple Silicon package and one managed Python fallback.
3. Add CLI chain planning and execution after single-model runners are stable.

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

The target CLI surface remains deliberately small: `search`, `pull`, `run`,
`list`, `info`, `lock`, `install`, and `rm`.
