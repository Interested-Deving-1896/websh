# Content Bundles

## Purpose

Websh should support articles and other works that have multiple concrete representations without weakening the filesystem-as-source-of-truth model. Language variants are the first use case: one work may have English and Korean markdown renditions. The same primitive should also support future renditions such as printable PDFs, HTML exports, appendices, source views, or other declared formats.

The primitive is a renderable directory bundle:

```text
content/writing/zk-proofs-from-a-compiler-perspective/
  _index.dir.json
  en.md
  en.meta.json
  ko.md
  ko.meta.json
```

The filesystem still contains real directories and real files. The semantic layer only changes how a directory with explicit bundle metadata is rendered, indexed, routed, and attested.

## Goals

- Keep the raw filesystem honest: no virtual article hidden outside the tree.
- Let standalone articles stay as standalone files.
- Treat declared variants as renditions of one work, not separate top-level content entries.
- Keep variant controls scoped to reader surfaces, not global site chrome.
- Make language just one variant dimension, not the bundle model itself.
- Make bundle behavior explicit in `_index.dir.json`; never infer it from filenames alone.
- Preserve raw terminal behavior: `ls`, `cd`, and `cat` should expose the real tree.
- Produce an implementation plan that can be executed in small plan, implementation, review cycles.

## Non-Goals

- No global site language switcher.
- No automatic machine translation.
- No flags as language icons.
- No implicit bundle creation from `en.md` and `ko.md` alone.
- No one-file multi-variant markdown format.

## Current Architecture Facts

Content sync starts in `crates/websh-cli/src/workflows/content/manifest.rs`. It walks content files, refreshes file sidecars, refreshes directory sidecars, and writes `content/manifest.json`.

Files and directories already share `NodeMetadata` in `crates/websh-core/src/domain/node_metadata.rs`. File sidecars are `<stem>.meta.json`; directory sidecars are `_index.dir.json`.

Runtime manifest parsing in `crates/websh-core/src/ports/manifest.rs` currently treats only `NodeKind::Directory` as directory-like. `GlobalFs` stores directories as `FsEntry::Directory { children, meta }`.

Route resolution in `crates/websh-core/src/engine/filesystem/routing.rs` resolves reserved routes, then an optional derived route index, then filesystem conventions. Directories currently become `ResolvedKind::Directory` and render as directory listings.

Reader metadata in `crates/websh-web/src/features/reader/meta.rs` is currently file-only through `file_meta_for_path`. A renderable directory will need a bundle-aware metadata projection.

Ledger and attestation discovery are currently file-oriented. A bundle-level content unit must group directory sidecar plus variant files intentionally.

## Core Model

There are three content node classes:

```text
standalone file       A normal renderable file, such as writing/foo.md.
ordinary directory    A navigable collection, such as writing/.
bundle directory      A renderable directory whose children include declared variants.
```

A directory becomes a bundle only when its `_index.dir.json` declares `kind: "bundle"`. Otherwise it remains an ordinary directory.

Standalone files continue to work exactly as they do today:

```text
content/writing/foo.md
content/writing/foo.meta.json
```

Bundles are used only when the author wants one work with multiple variants or related work-local assets:

```text
content/writing/foo/
  _index.dir.json
  en.md
  ko.md
  print.pdf
  cover.png
```

The bundle directory is the content unit. Declared variants are alternate renditions of that same unit. Other files inside the directory can be assets used by those variants without becoming content items themselves.

## Metadata Schema

Add `Bundle` to `NodeKind` and treat it as directory-like in manifest parsing and filesystem assembly.

Example bundle sidecar:

```json
{
  "schema": 1,
  "kind": "bundle",
  "authored": {
    "title": "Zero-Knowledge Proofs, from a Compiler Perspective",
    "description": "How programs become relations, constraints, traces, and proofs.",
    "date": "2026-05-15",
    "tags": ["zk", "compilers", "zero-knowledge", "systems"]
  },
  "bundle": {
    "default_variant": "en",
    "variants": [
      {
        "id": "en",
        "path": "en.md",
        "label": "English",
        "locale": "en"
      },
      {
        "id": "ko",
        "path": "ko.md",
        "label": "한국어",
        "locale": "ko"
      },
      {
        "id": "print",
        "path": "print.pdf",
        "label": "PDF",
        "media_type": "application/pdf"
      }
    ]
  }
}
```

Example variant frontmatter:

```yaml
---
title: "컴파일러 관점에서 보는 영지식 증명"
description: "프로그램이 relation, constraint, trace, proof가 되는 과정을 컴파일러 관점에서 설명합니다."
language: ko
---
```

Recommended core types:

```rust
pub struct BundleMetadata {
    pub default_variant: String,
    pub variants: Vec<BundleVariant>,
}

pub struct BundleVariant {
    pub id: String,
    pub path: String,
    pub label: String,
    pub locale: Option<String>,
    pub media_type: Option<String>,
}
```

`Fields` should gain `language: Option<String>` so standalone files and variant files can declare a content language. Bundle relationships should live on the parent `bundle` block, not as ad hoc `translation_of` links between siblings. Non-language renditions can use `media_type`, renderer metadata, or variant labels instead of `language`.

The `bundle` block is top-level `NodeMetadata`, not an authored display field. Directory sidecar sync must therefore preserve `existing.bundle` explicitly. For bundle directories, both top-level `kind` and `derived.kind` should remain `Bundle`; filesystem code should use `NodeKind::is_directory_like()` rather than relying on `kind == Directory`.

## Routing

Use path routes, not query parameters. The app already uses hash routing, and markdown headings need anchor-like behavior inside the reader.

Recommended routes:

```text
#/writing/foo        bundle default variant
#/writing/foo/en     explicit English markdown variant
#/writing/foo/ko     explicit Korean markdown variant
#/writing/foo/print  explicit PDF variant
```

The route node for all three is the bundle directory. The selected variant is route state:

```text
request path:  /writing/foo/print
bundle path:   /writing/foo
variant id:    print
variant path:  /writing/foo/print.pdf
```

This requires a new route result and render intent shape:

```rust
ResolvedKind::Bundle

RenderIntent::BundleVariant {
    bundle_path: VirtualPath,
    variant_id: String,
    variant_path: VirtualPath,
}
```

`GlobalFs::build_render_intent` should become bundle-aware and use the filesystem tree to select the default or requested variant. The current free function only receives `RouteResolution`, which is not enough to validate a bundle and select a child file.

The web reader must also carry the distinction through its local intent type:

```rust
ReaderIntent::BundleVariant {
    bundle_path: VirtualPath,
    variant_id: String,
    variant_path: VirtualPath,
}
```

Reader document loading reads from `variant_path`. Reader identity, site chrome, breadcrumbs, metadata merge, and attestation use `bundle_path`.

Direct file routes remain raw filesystem routes:

```text
#/writing/foo/ko.md
```

UI must not generate those links for bundle navigation. Reader variant controls use only canonical bundle variant routes:

```text
#/writing/foo/ko
```

## Reader UX

The variant switcher belongs in the reader title area, not in global chrome and not inside the markdown body renderer.

Placement:

- `ReaderShell` continues to own document chrome.
- `TitleBlock` receives bundle variant state.
- The switcher appears under the title or as a compact `Variants` row in the metadata table.

Recommended visual form:

```text
Variants    English  한국어  PDF
```

Behavior:

- Hide the switcher when a file has no variants.
- Mark the active variant as selected.
- Link inactive variants to canonical bundle variant routes.
- Disable or hide variant switching while edit mode has unsaved changes.
- Do not use flags.
- Do not label titles with suffixes like `(Korean)`.

Metadata display:

- The title should come from the selected variant if present.
- Date, tags, and high-level description should come from the bundle by default.
- Variant description may override the reader abstract/caption for that rendition.
- Word count and reading time should come from the selected variant file.
- The footer attestation should refer to the bundle route when the bundle is signed as one content unit.

## Content-Unit Projection

Bundle directories should collapse to one visible item in high-level surfaces:

- home recent feed,
- writing directory list,
- ledger summary surfaces,
- category counts.

Raw filesystem surfaces should not collapse them:

- terminal `ls`,
- terminal `cat`,
- direct directory listings if explicitly opened as filesystem views.

For user-facing writing lists, a bundle row should show available variant chips:

```text
Zero-Knowledge Proofs, from a Compiler Perspective      English  한국어  PDF
2026-05-15 · zk · compilers
```

If a bundle has no valid default variant, render it as a broken bundle row with a clear missing-default state rather than silently falling back.

Ledger surfaces can only become structurally bundle-aware after the ledger generator groups bundle variants into one block. Before that grouping lands, home/recent/category projections may collapse bundles, but ledger counts should continue to reflect actual ledger blocks.

The projection rule is general:

```text
standalone renderable file -> one content item
bundle directory           -> one content item
declared variant child     -> rendition of its parent bundle, not a separate content item
ordinary directory         -> collection, not a content item unless its metadata says bundle
unlisted child asset       -> support file, not a content item
```

This rule is what makes the model future-proof. Translations, PDFs, HTML exports, and other renditions are all handled by `bundle.variants`; only declared variants appear in the switcher.

## Ledger And Attestation Policy

The recommended policy is bundle-level attestation.

A bundle is one content unit. Its ledger subject route is the bundle root:

```text
route: /writing/foo
kind: bundle
```

The signed content file list should include:

```text
content/writing/foo/_index.dir.json
content/writing/foo/en.md
content/writing/foo/en.meta.json
content/writing/foo/ko.md
content/writing/foo/ko.meta.json
content/writing/foo/print.pdf
content/writing/foo/print.meta.json
```

This makes the bundle route verify all variants. Variant routes such as `/writing/foo/ko` or `/writing/foo/print` should show the same bundle attestation, because the selected variant is included in the bundle subject.

Bundle routes should use a `Bundle` attestation subject kind. Do not sign bundle routes as `Page`; that would keep two content concepts with one label and make the architecture harder to reason about.

Adding `Subject::Bundle` changes canonical messages and generated signatures. That churn is expected as part of the migration.

## Migration Strategy

Current separate files:

```text
content/writing/zk-proofs-from-a-compiler-perspective.md
content/writing/zk-proofs-from-a-compiler-perspective-ko.md
```

Target bundle:

```text
content/writing/zk-proofs-from-a-compiler-perspective/
  _index.dir.json
  en.md
  en.meta.json
  ko.md
  ko.meta.json
```

Route model after migration:

- `/writing/zk-proofs-from-a-compiler-perspective` is the bundle default.
- `/writing/zk-proofs-from-a-compiler-perspective/ko` is the Korean variant.
- `/writing/zk-proofs-from-a-compiler-perspective-ko` is removed.

There should be no route for the removed sibling path. Existing content should be fully migrated into the bundle structure.

Generated artifacts expected to change:

- `content/manifest.json`
- `content/.websh/ledger.json`
- `assets/crypto/attestations.json`
- relevant `_index.dir.json` child counts

Signature reuse should not be assumed. The content unit changes from separate files to one bundle.

## Schema Change Notes

Adding `kind: "bundle"` and `bundle` fields is a schema change. Existing runtimes with strict metadata parsing will reject the new manifest and sidecars.

This is acceptable only as a coordinated repo change where the Rust code, generated content, and deployed assets move together.

Browser commits currently update content and manifest state, but sidecar generation is CLI-owned. Bundle editing should initially remain CLI/local-authoring only unless browser commit support is extended to write or preserve directory sidecars safely.

## Implementation Slices

Each slice should run as:

```text
Plan -> Implement -> Review with GPT-5.5 high/xhigh -> Fix review findings -> Verify
```

The review agent should be given the slice goal, changed files, and the relevant invariants below. Reviews should focus on regressions, missing tests, schema drift, and route or attestation inconsistencies.

### Slice 1: Domain Schema

Plan:

- Add `NodeKind::Bundle`.
- Add `BundleMetadata` and `BundleVariant` domain types.
- Add `bundle: Option<BundleMetadata>` to `NodeMetadata`.
- Add `language: Option<String>` to `Fields`.
- Add optional non-language variant fields such as `media_type` on `BundleVariant`.
- Add helpers such as `NodeKind::is_directory_like()` and `NodeMetadata::is_bundle()`.
- Add compile-only arms and default handling in all exhaustive `NodeKind` matches and explicit `Fields` construction sites, so this slice remains buildable before bundle behavior exists.

Implementation touchpoints:

- `crates/websh-core/src/domain/node_metadata.rs`
- `crates/websh-core/src/domain/manifest.rs`
- `crates/websh-core/src/engine/filesystem/routing.rs`
- `crates/websh-web/src/shared/components/file_meta.rs`
- `crates/websh-web/src/features/reader/title_block.rs`
- `crates/websh-cli/src/workflows/content/frontmatter.rs`
- `crates/websh-cli/src/workflows/content/sidecar.rs`
- `tests/fixtures/manifest_golden.json`

Review prompt:

```text
Review the schema changes for content bundles. Check serde behavior inside this repo, unknown-field behavior, default serialization, helper semantics, and fixture drift. Do not suggest broad refactors.
```

Verification:

```bash
cargo test -p websh-core domain::node_metadata
cargo test -p websh-core domain::manifest
```

### Slice 2: Manifest Parsing And GlobalFs Import

Plan:

- Treat `NodeKind::Bundle` as directory-like in manifest parsing.
- Preserve bundle metadata through `ScannedDirectory`.
- Ensure directory import/export keeps bundle metadata intact.
- Keep raw filesystem behavior unchanged.

Implementation touchpoints:

- `crates/websh-core/src/ports/manifest.rs`
- `crates/websh-core/src/engine/filesystem/snapshot.rs`
- `crates/websh-core/src/domain/filesystem.rs`
- `crates/websh-core/src/engine/filesystem/global_fs/*`

Review prompt:

```text
Review manifest and GlobalFs changes for directory-like bundle handling. Verify bundles remain real directories, files remain real children, and ordinary directories are unaffected.
```

Verification:

```bash
cargo test -p websh-core ports::manifest
cargo test -p websh-core engine::filesystem
```

### Slice 3: CLI Sidecar And Manifest Sync

Plan:

- Stop hard-resetting bundle directory sidecars to `kind: "directory"`.
- Preserve top-level `existing.bundle` across `content manifest`.
- Preserve top-level bundle kind and write `derived.kind = Bundle` for bundle directories.
- Validate bundle variant paths are relative, non-empty, unique, and inside the bundle directory.
- Validate default variant exists.
- Keep sync byte-stable.

Implementation touchpoints:

- `crates/websh-cli/src/workflows/content/sidecar.rs`
- `crates/websh-cli/src/workflows/content/manifest.rs`
- `crates/websh-cli/src/workflows/content/frontmatter.rs`
- `crates/websh-cli/src/workflows/content/files.rs`

Review prompt:

```text
Review CLI content sync for bundle sidecars. Check deterministic output, preservation of authored fields, validation coverage, and failure modes for malformed bundles.
```

Verification:

```bash
cargo test -p websh-cli content
cargo run --quiet -p websh-cli -- content manifest
git diff -- content/manifest.json
```

### Slice 4: Routing And Render Intent

Plan:

- Add `ResolvedKind::Bundle`.
- Add `RenderIntent::BundleVariant`.
- Add the corresponding web `ReaderIntent::BundleVariant`.
- Resolve `/writing/foo` to the default variant for bundle directories.
- Resolve `/writing/foo/ko` to the `ko` variant when declared by the parent bundle.
- Keep `/websh` and normal file conventions unchanged.
- Decide direct-file behavior for `/writing/foo/ko.md`: render as raw file or canonicalize to `/writing/foo/ko`.

Implementation touchpoints:

- `crates/websh-core/src/engine/filesystem/routing.rs`
- `crates/websh-core/src/engine/filesystem/intent.rs`
- `crates/websh-core/src/engine/filesystem/content_routes.rs`
- `crates/websh-core/src/engine/filesystem/global_fs/mod.rs`
- `crates/websh-web/src/features/router.rs`
- `crates/websh-web/src/features/reader/intent.rs`

Review prompt:

```text
Review bundle routing. Check route precedence, collision behavior with foo.md vs foo/, default variant selection, explicit variant routes, and existing route tests.
```

Verification:

```bash
cargo test -p websh-core routing
cargo check -p websh-web --target wasm32-unknown-unknown
```

### Slice 5: Reader Bundle UX

Plan:

- Load selected variant content while keeping bundle path as the document identity.
- Combine bundle metadata and variant metadata.
- Add a scoped variant switcher in the title/meta area.
- Hide switcher for standalone files.
- Disable switcher while dirty edit state exists.

Implementation touchpoints:

- `crates/websh-web/src/features/reader/mod.rs`
- `crates/websh-web/src/features/reader/document.rs`
- `crates/websh-web/src/features/reader/meta.rs`
- `crates/websh-web/src/features/reader/shell.rs`
- `crates/websh-web/src/features/reader/title_block.rs`
- `crates/websh-web/src/features/reader/reader.module.css`

Review prompt:

```text
Review reader bundle UX. Check metadata precedence, variant switcher visibility, active state, edit-mode behavior, accessibility labels, and responsive layout.
```

Verification:

```bash
cargo check -p websh-web --target wasm32-unknown-unknown
npm run lint:css
```

### Slice 6: Bundle-Aware Content-Unit Projection

Plan:

- Introduce a shared content-item projection for high-level surfaces.
- Collapse each bundle directory to one projected content item.
- Exclude declared variant children from recursive item collection.
- Keep unlisted bundle assets out of high-level content item counts.
- Count bundle directories as one content item.
- Show variant chips from `bundle.variants` when variants exist.
- Keep terminal and raw filesystem listing unchanged.
- Leave ledger surfaces unchanged until bundle ledger grouping lands.

Implementation touchpoints:

- `crates/websh-web/src/features/home/model.rs`
- `crates/websh-web/src/features/home/mod.rs`
- `crates/websh-core/src/engine/shell/executor/read.rs`

Review prompt:

```text
Review bundle-aware content-unit projection. Check duplicate suppression, category counts, recent item sorting, declared variant handling, bundle asset exclusion, and that terminal filesystem behavior remains raw.
```

Verification:

```bash
cargo check -p websh-web --target wasm32-unknown-unknown
cargo test -p websh-core shell
```

### Slice 7: Ledger And Attestation Grouping

Plan:

- Group bundle files into one ledger block.
- Add a shared `bundle_content_paths` helper that reads declared variants, includes `_index.dir.json`, includes variant sidecars, validates paths, and feeds both ledger and attestation discovery.
- Use the bundle route as the subject route.
- Include `_index.dir.json` and all declared variant files and sidecars in the signed content file set.
- Introduce `Subject::Bundle`; do not sign bundle routes as `Page`.
- Ensure removed sibling files no longer emit routes, ledger blocks, or attestation subjects.

Implementation touchpoints:

- `crates/websh-cli/src/workflows/content/ledger.rs`
- `crates/websh-cli/src/workflows/content/files.rs`
- `crates/websh-cli/src/workflows/attest/discover.rs`
- `crates/websh-cli/src/workflows/attest/subject/*`
- `crates/websh-core/src/engine/attestation/subject.rs`
- `crates/websh-core/src/engine/attestation/ledger/*`
- `crates/websh-site/src/artifacts.rs`

Review prompt:

```text
Review bundle ledger and attestation grouping. Check canonical file ordering, hash stability, route uniqueness, signature reuse behavior, and whether variant content is actually bound by the bundle subject.
```

Verification:

```bash
cargo test -p websh-core attestation
cargo test -p websh-cli attest
cargo run --quiet -p websh-cli -- attest build
```

### Slice 8: Ledger UI After Grouping

Plan:

- Make ledger models display grouped bundle blocks as one unit.
- Show available variant chips when the ledger entry is a bundle.
- Keep bundle route and footer route aligned with the signed subject route.
- Ensure counts now reflect grouped ledger blocks, not variant files.

Implementation touchpoints:

- `crates/websh-web/src/features/ledger/model.rs`
- `crates/websh-web/src/features/ledger/mod.rs`
- `crates/websh-web/src/shared/components/signature_footer.rs`

Review prompt:

```text
Review ledger UI bundle rendering after ledger grouping. Check route alignment with attestation subjects, counts, variant chip display, and unchanged behavior for non-bundle ledger blocks.
```

Verification:

```bash
cargo check -p websh-web --target wasm32-unknown-unknown
```

### Slice 9: Content Migration

Plan:

- Move paired translated articles into bundle directories.
- Remove title suffixes like `(Korean)`.
- Add bundle sidecars with explicit variants.
- Delete replaced sibling translation files.
- Do not keep any route entry for removed sibling paths.
- Regenerate manifest, ledger, and attestations.

Implementation touchpoints:

- `content/writing/*`
- `content/writing/_index.dir.json`
- `content/manifest.json`
- `content/.websh/ledger.json`
- `assets/crypto/attestations.json`

Review prompt:

```text
Review content migration. Check routes, metadata quality, removed sibling entries, generated artifacts, and whether the filesystem tree matches the bundle design document.
```

Verification:

```bash
cargo run --quiet -p websh-cli -- content manifest
cargo run --quiet -p websh-cli -- attest build
```

### Slice 10: Browser Verification

Plan:

- Add or update e2e coverage for bundle routes.
- Verify root-host and `/ipfs/<cid>/` hash routing.
- Check variant switcher navigation.
- Check no duplicate recent feed rows.
- Check console and asset 404 failures.

Implementation touchpoints:

- `tests/e2e/websh.spec.js`
- `tests/e2e/mempool.spec.js` if edit behavior changes
- `playwright.config.js`

Review prompt:

```text
Review browser tests for content bundles. Check that tests cover default variant routing, explicit variant routing, switcher behavior, non-language variants, and non-duplication in high-level surfaces.
```

Verification:

```bash
npm run e2e
```

## Full Verification Gate

After all slices land:

```bash
just verify
```

For local iteration, use the focused gates in `docs/architecture/verification.md`.

## Open Decisions

The implementation should settle these before Slice 7:

- Support only markdown/PDF variants in the first version, or allow any reader-supported variant file.
- Decide whether direct file variant routes like `#/writing/foo/ko.md` render as raw files or are rejected.
- Whether browser authoring should support bundle sidecar edits in the first version.

Recommended defaults:

- Add `NodeKind::Bundle`.
- Add `Subject::Bundle`.
- Use explicit declared variant files such as `en.md`, `ko.md`, and `print.pdf`.
- Use bundle-level attestation.
- Render direct variant file routes as raw files, but generate UI links only to canonical bundle variant routes.
- Keep browser bundle authoring out of the first implementation.
