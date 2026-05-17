# Current Architecture

This is the authoritative architecture document for the current repository.

## System Shape

Websh is a verifiable personal archive backed by a Rust/WASM runtime and a browser-native virtual filesystem. It assembles content manifests and runtime mounts into one canonical tree rooted at `/`, renders that tree through Leptos, and offers reader, ledger, and terminal views for navigation and staged writes.

The workspace has four crates:

- `websh-core`: shared domain types, filesystem, shell, runtime orchestration, mempool helpers, attestation primitives, and storage ports.
- `websh-site`: deployed-site identity and policy constants that are not generic engine logic.
- `websh-cli`: native command adapter for content generation, attestation, deploy, mempool, and mount workflows.
- `websh-web`: Leptos/WASM browser adapter, runtime services, feature views, platform APIs, and styling.

The dependency direction is:

```text
websh-cli  -> websh-core, websh-site
websh-web  -> websh-core, websh-site
websh-site -> websh-core
websh-core -> external libraries only
```

`websh-cli` and `websh-web` must not depend on each other.

## Core Public API

`websh-core::engine` is private. External crates use the public facades:

- `websh_core::domain`
- `websh_core::filesystem`
- `websh_core::runtime`
- `websh_core::shell`
- `websh_core::mempool`
- `websh_core::attestation`
- `websh_core::crypto`
- `websh_core::ports`
- `websh_core::support`

New shared behavior should enter through one of these facades. Contextual error
types are exported from the facade that owns the capability; there is no global
`websh_core::errors` facade. Do not add new cross-crate consumers of `engine`.

## Boundaries

`websh-core` owns pure contracts and cross-target behavior. It should not reach into browser APIs, process APIs, GitHub CLI process execution, or Leptos signals.

`websh-cli` owns host processes and filesystems. Clap command modules should stay thin and delegate use-case logic into `workflows`; process adapters live in `infra`.

`websh-web` owns browser state, IndexedDB, local/session storage, wallet APIs, DOM APIs, object URLs, fetch cancellation, and Leptos component state. Feature modules should call `AppContext` and `RuntimeServices` instead of reading browser storage directly.

`websh-site` owns stable deployed identity: public key material, expected fingerprints, acknowledgement artifacts, site copy/policy, and content fixtures that are specific to this deployment.

## Path Model

All engine paths are `VirtualPath` values. They are canonical absolute paths and reject relative or non-canonical input at construction and deserialization.

Runtime overlay paths are centralized through `runtime_state_root()` and `is_runtime_overlay_path()`. Shell writes and exports must reject runtime overlay mutation.

## URL Model

The deployed browser app is a static, hash-routed application. The canonical root URL is `/#/`; internal routes use the same model, for example `/#/ledger`, `/#/websh`, and `/#/writing/example`.

Generated in-app links are hash-only (`#/ledger`, `#/writing/example`) so they preserve the current document base under path-gateway deployments such as `/ipfs/<cid>/`. Direct external links may still include the leading `/` on root hosts, but clean deep paths such as `/writing/example` are best-effort only and require the host to serve `index.html` for unknown paths.

## Runtime Model

The web app boots a `RuntimeLoad`:

1. Read bundled `content/manifest.json`.
2. Read declared runtime mounts.
3. Assemble a `GlobalFs`.
4. Start remote mount scans.
5. Hydrate browser runtime state and drafts.
6. Derive the rendered view filesystem from base filesystem, staged `ChangeSet`, wallet state, and runtime state.

Drafts persist in IndexedDB after successful hydration. The browser writes pathwise draft deltas so a single edited file does not rewrite every draft record.

## Commit Model

Writes are staged as canonical `ChangeSet` entries. Commit preparation:

1. validates staged paths are inside one mount root,
2. rejects unsupported binary changes,
3. normalizes directory deletes and descendant changes,
4. expands directory deletes to concrete file deletions,
5. builds a backend-neutral `CommitDelta`,
6. submits through a strict mount-root `StorageBackend`.

GitHub commits use compare-and-swap with the expected remote head.

## Build And Attestation

Trunk pre-build hooks run in this order:

1. `stylance --output-file assets/bundle.css crates/websh-web`
2. `cargo run --quiet -p websh-cli -- content manifest`
3. `cargo run --quiet -p websh-cli -- attest build`

`attest build` is release-profile aware. It skips non-release Trunk profiles unless `--force` is passed. `WEBSH_NO_SIGN=1` disables signing while still refreshing pending subjects.

Generated content artifacts include `content/manifest.json`, `content/ledger.json`, sidecar metadata, and `assets/crypto/attestations.json`.

## Verification

The local gate is `just verify`. The command list is mirrored in [verification.md](verification.md) and checked by `npm run docs:drift`.
