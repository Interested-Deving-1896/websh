# Crates And Ownership

## Workspace Members

| Crate | Target | Owns | Must Not Own |
|---|---|---|---|
| `websh-core` | host + wasm | domain, public facades, filesystem, shell parser/executor, runtime coordination, mempool helpers, attestation primitives, ports | browser APIs, process execution, CLI argument parsing, Leptos signals |
| `websh-site` | host + wasm | deployed identity, public key constants, acknowledgement data, site-specific policy/copy | generic engine rules, browser state, command workflows |
| `websh-cli` | host | Clap adapters, workflows, process/filesystem/GitHub CLI adapters, deploy tooling | Leptos UI, browser persistence, generic domain rules |
| `websh-web` | wasm | Leptos app, AppContext, runtime services, IndexedDB/localStorage/sessionStorage adapters, DOM/fetch/wallet integration, feature views | host process execution, generic engine internals |

## Dependency Rules

Allowed:

```text
websh-site -> websh-core
websh-cli  -> websh-core
websh-cli  -> websh-site
websh-web  -> websh-core
websh-web  -> websh-site
```

Rejected:

```text
websh-core -> any workspace crate
websh-web  -> websh-cli
websh-cli  -> websh-web
```

## Public Core Facades

External crates import shared behavior from:

- `websh_core::domain`
- `websh_core::filesystem`
- `websh_core::runtime`
- `websh_core::shell`
- `websh_core::mempool`
- `websh_core::attestation`
- `websh_core::crypto`
- `websh_core::ports`
- `websh_core::support`

Error types are exported from the owning facade (`domain`, `filesystem`,
`runtime`, `attestation`, `ports`, and so on). There is no cross-context
`websh_core::errors` facade.

`websh_core::engine` is private. If a consumer needs something under `engine`, expose a narrow item through the correct facade instead of making `engine` public.

## Review Checklist

- New shared types start in `domain` only if they are stable data contracts.
- New storage behavior goes behind `ports` before a concrete backend depends on it.
- New CLI commands put parsing in `commands`, use cases in `workflows`, and process execution in `infra`.
- New web features keep browser APIs in `runtime` or `platform`, not directly in pure feature model code.
