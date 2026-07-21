# Development notes

This document contains implementation details for maintainers. Users should be
able to install and operate the server from the main README alone.

## Scope

`kie-mcp` wraps Kie Market image and video generation through the task API. It
does not aim to cover chat, audio-only generation, webhooks, legacy endpoints,
or every model-specific schema.

The public MCP surface stays deliberately small:

- generation is split into image and video tools;
- common inputs are top-level tool fields;
- uncommon model fields pass through the open `input` object;
- local references are uploaded automatically;
- completed media is downloaded before the tool returns.

Kie remains the source of truth for model-specific validation and pricing.

## Architecture

| Path | Responsibility |
| --- | --- |
| `src/mcp.rs` | Tool schemas, MCP results, and stdio lifecycle. |
| `src/config.rs` | Environment configuration and startup validation. |
| `src/kie/catalog.rs` | Model types, lookup, matching, and request-profile helpers. |
| `src/kie/catalog/models.rs` | Data-only model IDs, aliases, media bindings, and common field mappings. |
| `src/kie/jobs.rs` | Model checks and Kie request assembly. |
| `src/kie/client.rs` | HTTP calls, uploads, polling, downloads, and error redaction. |
| `src/kie/normalize.rs` | Extraction of result and poster URLs from Kie responses. |
| `src/media.rs` | Local filenames, extensions, and Markdown previews. |

A generation request follows this path:

1. Resolve the requested catalog model and validate image/video kind.
2. Validate and upload any local reference files.
3. Merge convenience fields and media URLs into Kie's `input` object.
4. Create one Kie task and poll `recordInfo` until it finishes.
5. Resolve and download result media into a task-specific directory.
6. Return structured data plus local Markdown previews.

Do not split these modules further unless a boundary has independent behavior
or repeated change pressure. File length alone is not a reason to add layers.

## Model catalog

The embedded catalog is sourced from <https://docs.kie.ai/llms.txt>. Its entries
live in `src/kie/catalog/models.rs`, separate from matching behavior. It is not a
copy of Kie's full schemas. Each entry stores only what the MCP needs for model
selection and common request assembly:

- canonical ID, display name, kind, and aliases;
- a simple media URL binding when one exists;
- common aspect ratio, resolution, and output format mappings.

Catalog refreshes are intentionally manual and can be done in a focused Codex
session. Change the data file, preserve exact-key uniqueness, and add or update a
representative request-assembly test for any changed binding. Do not add full
model schemas to the binary; uncommon fields belong in `input` and in Kie's own
documentation.

## Configuration reference

| Variable | Default | Purpose |
| --- | --- | --- |
| `KIE_API_KEY` | unset | Required for live API calls. |
| `KIE_MCP_API_BASE` | `https://api.kie.ai` | Kie task and credit API base URL. |
| `KIE_MCP_UPLOAD_BASE` | `https://kieai.redpandaai.co` | Kie upload API base URL. |
| `KIE_MCP_OUTPUT_DIR` | `output/kie` | Download root. |
| `KIE_MCP_TIMEOUT_SECS` | `900` | Overall generation polling deadline. |
| `KIE_MCP_HTTP_TIMEOUT_SECS` | `300` | Timeout for each HTTP request. |
| `KIE_MCP_MAX_UPLOAD_BYTES` | `536870912` | Maximum accepted local file size. |
| `KIE_MCP_INPUT_ROOTS` | unset | Platform-separated allowlist of upload roots. |

Configured values are validated at startup. Base URLs must be `http` or `https`
without query strings or fragments. Numeric values must be positive integers.

Relative output paths are resolved from the server process working directory.
Input roots are canonicalized before comparison. Uploads must be regular files
whose extension maps to an image or video MIME type.

## Debug CLI

The debug commands exercise the same client without MCP:

```bash
cargo run -- debug models --media-type image --query banana
cargo run -- debug credits
cargo run -- debug upload ./image.png
cargo run -- debug create --model MODEL_ID --input input.json
cargo run -- debug wait TASK_ID --download --media-type image
```

`debug create` expects `input.json` to contain the Kie input object, including
`prompt`. Set `RUST_LOG=debug` for diagnostics; logs go to stderr so they do not
corrupt MCP stdio messages.

## Runtime behavior and limits

- Concurrent generation calls are supported. Every task keeps its own task ID, and
  download directories include that ID even when callers reuse the same
  `output_name`; the mock suite locks this with three simultaneous generations.
- Polling retries network errors and HTTP `408`, `429`, and `5xx` responses with
  a delay that grows from two to ten seconds.
- The upload cache lasts for one server process and is keyed by canonical path,
  file size, and modification time.
- Result downloads validate `http`/`https` URLs and reject obvious local or
  private host literals. This is a guardrail, not a network sandbox: redirects
  and DNS resolution are not independently pinned.
- Uploads and downloads are currently buffered in memory. Upload size is capped;
  result download size is not yet capped.
- Result extraction supports common Kie fields and retains a generic fallback
  for model response shapes not represented by the catalog.
- The local catalog can lag behind Kie's availability and capabilities. Raw
  model IDs are accepted only when their media kind can be inferred safely.

## Verification

Tests are mock-only and do not require `KIE_API_KEY`.

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build --release
```

The CI workflow runs formatting, strict Clippy, and the test suite on stable
Rust. Keep tests focused on public request behavior and observed Kie response
shapes rather than internal call structure.
