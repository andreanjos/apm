# Registry Architecture

This document defines the long-term direction for the apm registry if apm
becomes the primary package manager for macOS audio plugins.

It is intentionally closer to the publishing model used by `apt` than to the
current source layout under `registry/plugins/<vendor>/*.toml`, but it does not copy Debian
repository structure literally. The goal is to preserve a human-editable source
tree while giving clients a compact, versioned, signed index to consume.

## Goals

- Support tens of thousands of plugins without making sync or search slow.
- Support multiple registries: official, community, private, and vendor-owned.
- Treat vendors and installer apps as first-class entities.
- Separate authoring layout from published client-facing metadata.
- Make schema evolution explicit and backwards-compatible.
- Enable trust features such as checksums, signing, and source policy.

## Non-Goals

- Reproduce Debian's exact `pool/` and `dists/` layout.
- Model Linux package management concepts that do not apply to audio plugins.
- Require a full registry rewrite before apm can continue to grow.

## Current Problems

The current structure is acceptable for a few hundred plugins:

```text
registry/
  plugins/<vendor>/*.toml
  bundles/*.toml
  installers.toml
```

It will become painful as scale increases:

- Even with vendor grouping, clients still think in terms of raw plugin files
  rather than published indexes.
- Vendor data, installer data, and plugin data are only partially normalized.
- Large syncs will eventually mean reading too many small files.

## Design Principles

1. Author once, publish many.
2. The source tree is for humans; published indexes are for clients.
3. Stable IDs matter more than filenames.
4. Shared entities should not be duplicated across plugin files.
5. Registry releases must be versioned and verifiable.

## Target Model

The registry should have two layers.

### 1. Authoring Layer

Human-edited source files grouped by entity type and vendor.

```text
registry-src/
  plugins/
    native-instruments/
      massive-x.toml
      kontakt.toml
    arturia/
      pigments.toml
      mini-v.toml
    waves/
      h-delay.toml
  installers/
    native-access.toml
    waves-central.toml
    ua-connect.toml
  vendors/
    native-instruments.toml
    arturia.toml
    waves.toml
  bundles/
    producer-essentials.toml
  schemas/
  tooling/
```

Key properties:

- Vendor subdirectories keep review scope local and predictable.
- Installers are individual records, not one expanding monolithic TOML file.
- Vendors become first-class records with aliases and metadata.
- Tooling can validate and generate published indexes from this source tree.

### 2. Published Layer

Generated, versioned, signed metadata optimized for apm clients.

```text
registry-dist/
  v1/
    release.toml
    checksums.txt
    index/
      plugins.json.zst
      installers.json.zst
      vendors.json.zst
      bundles.json.zst
      search.json.zst
    shards/
      plugins/
        a.json.zst
        b.json.zst
        native-instruments.json.zst
        waves.json.zst
      bundle-ids/
        a.json.zst
        b.json.zst
```

Key properties:

- Clients fetch release metadata and compact indexes, not every source file.
- The published layer can evolve without forcing authoring changes.
- Shards allow incremental sync and bounded memory use.

## Why This Resembles `apt`

`apt` separates editable package metadata from client-facing generated indexes.
That is the right lesson to copy.

What apm should borrow from `apt`:

- a release manifest
- checksums/signatures
- generated indexes instead of walking raw package files
- incremental updates

What apm should not borrow:

- Debian distribution/component/architecture complexity
- `.deb` artifact pool semantics
- mirror-oriented layout decisions that do not fit plugin metadata

## First-Class Entities

The registry should evolve toward these normalized entities:

### Vendor

- stable ID
- display name
- aliases
- homepage
- support URL
- known installer IDs

### Installer

- stable ID
- display name
- vendor ID
- app detection paths
- download URL
- homepage
- launch hints

### Plugin

- stable ID
- slug
- vendor ID
- product name
- categories/tags
- supported install methods
- bundle ID patterns

### Release

- plugin ID
- version
- release date
- supported formats
- compatibility constraints

### Artifact

- release ID
- format
- package type
- URL
- checksum
- bundle path

This model leaves room for:

- vendor-managed products
- direct-download freeware
- account-gated downloads
- bundle/family products
- future license or entitlement metadata

## Stable Identity

Today the slug is the practical identifier. Long term, that is not enough.

Each major entity should gain a stable internal ID. Slugs can still exist for
CLI ergonomics, but IDs should be the canonical join key across indexes.

## Schema Versioning

Every published release must declare a `schema_version`.

That version gates:

- how apm parses published indexes
- whether older clients can continue to function
- how migration logic is applied

Schema versioning should exist at the published layer first, even if source
authoring remains TOML-based for a while.

## Trust Model

If apm becomes a primary installer, registry trust becomes critical.

The published layer should support:

- release manifest checksums
- signed release metadata
- source identity and trust policy
- explicit rejection of malformed or downgraded metadata

This is the closest analog to `apt`'s strongest ideas.

## Sync Model

The ideal client flow is:

1. Fetch `release.toml`
2. Verify checksum/signature
3. Download changed indexes or shards only
4. Replace the local cache atomically

The client should not need to parse thousands of raw TOML files on each sync.

## Recommended Migration Path

This should happen in phases, not as one rewrite.

### Phase 1: Organize Source Tree

- Move `registry/plugins/*.toml` into vendor subdirectories.
- Split `installers.toml` into per-installer files or keep it generated from a
  normalized source folder.
- Add a `vendors/` source directory.

### Phase 2: Generate Indexes

- Introduce a build/publish script that reads source files and emits:
  - `plugins.json`
  - `installers.json`
  - `vendors.json`
  - `bundles.json`
- Keep current raw TOML loading working during the transition.

### Phase 3: Teach Clients to Prefer Published Indexes

- Make `apm sync` fetch generated indexes first.
- Fall back to raw source trees only for local development registries.

### Phase 4: Add Sharding

- Split plugin indexes by vendor or prefix once index size justifies it.

### Phase 5: Add Release Signing

- Sign release metadata.
- Verify signatures by default for trusted registries.

## Immediate Recommendation

Do not do a disruptive registry rewrite yet.

The next practical step should be:

1. Introduce vendor subdirectories in the source tree.
2. Add registry tooling that generates compact published indexes.
3. Update `apm-core` so the runtime contract is the generated index, not raw
   authoring files.

That path keeps contributor ergonomics reasonable today while preparing apm for
much larger scale.
