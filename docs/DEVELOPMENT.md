# Development notes

This document contains implementation details for maintainers. Users should be
able to install and operate the server from the main README alone.

## Scope

`kie-mcp` wraps Kie Market image and video generation through the task API. It
also supports five preparation operations tied to Grok Image 2, Gemini Omni
Video, and OmniHuman 1.5. Gemini's two profile endpoints are included only
because their IDs feed video generation. The project does not cover chat,
standalone audio generation, webhooks, legacy endpoints, or every model schema.

The public MCP surface stays deliberately small:

- generation is split into image and video tools;
- common inputs are top-level tool fields;
- uncommon model fields pass through the open `input` object;
- local references are uploaded automatically;
- completed media is downloaded before the tool returns;
- structured preparation results use dedicated tools instead of pretending to
  be generated media.

Kie's route OpenAPI remains the contract source. The MCP enforces a conservative
local subset and leaves undocumented or media-dependent checks to Kie. Kie also
remains the source of truth for pricing.

## Architecture

| Path | Responsibility |
| --- | --- |
| `src/mcp.rs` | Tool schemas, MCP results, and stdio lifecycle. |
| `src/config.rs` | Environment configuration and startup validation. |
| `src/kie/catalog.rs` | Embedded compatibility profiles, lookup, matching, and request helpers. |
| `src/kie/catalog/live.rs` | Live index and route downloads, OpenAPI parsing, schema output, and fallback merging. |
| `src/kie/catalog/validation.rs` | Conservative recursive validation for authoritative downloaded input schemas. |
| `src/kie/catalog/models.rs` | Data-only model IDs, aliases, media bindings, and common field mappings. |
| `src/kie/jobs.rs` | Model checks and Kie request assembly. |
| `src/kie/client.rs` | HTTP calls, uploads, polling, downloads, and error redaction. |
| `src/kie/normalize.rs` | Extraction of result and poster URLs from Kie responses. |
| `src/kie/operations.rs` | Typed Gemini profiles and structured Grok/OmniHuman results. |
| `src/media.rs` | Local filenames, extensions, and Markdown previews. |

A generation request follows this path:

1. Resolve the requested model against the live route catalog and embedded
   compatibility profiles, then validate its image/video kind.
2. Merge convenience fields and placeholder URLs into a preview input. Validate
   it against an authoritative downloaded route schema before uploading files.
3. Validate and upload local reference files, then assemble and validate the
   final Kie input.
4. Create one Kie task and poll `recordInfo` until it finishes.
5. Resolve and download result media into a task-specific directory.
6. Return structured data plus local Markdown previews.

The three asynchronous preparation operations share task creation and polling
with generation, then parse `resultJson` into typed results. Segment and subject
masks keep their remote URLs and receive best-effort local previews. The two
Gemini Omni profile endpoints are synchronous and return reusable IDs.

Do not split these modules further unless a boundary has independent behavior
or repeated change pressure. File length alone is not a reason to add layers.

## Model catalog

The runtime catalog starts at <https://docs.kie.ai/llms.txt>. On first use, it
downloads that index and finds same-origin English Kie Market image and video
routes. It downloads only the route documents needed by the current model query
or generation request. An unfiltered `kie_models` call reads all matching routes
with at most eight requests in flight. Index and route load results stay cached
for the server process.

Each route page contains an OpenAPI YAML block. The parser resolves local schema
references and extracts the `input` schema from
`POST /api/v1/jobs/createTask`. A contract is authoritative for local validation
only when its `model` schema identifies exactly one model, that ID agrees with
the route URL, title, or embedded profile, and its `input` is object-shaped.
This identity check matters because Kie's current catalog contains route pages
whose singleton model enum names another route. Profile routes such as Gemini
Omni Audio and Character do not use `createTask`, so the catalog ignores them
instead of reporting failed schemas.

`kie_models` returns the recursive input schema. The normal response removes
examples, descriptions, and `x-*` vendor fields but keeps standard schema
structure and constraints. `include_descriptions=true` retains descriptions.
The response labels every entry with `catalog_source` and `schema_status`, and
the top level reports whether the live load was complete, partial, or replaced
by the embedded fallback.

The validator enforces only constraints that can be checked without inspecting
remote media:

- JSON types, `const`, enums, and required object fields;
- string lengths and numeric bounds;
- array and object size limits, unique array items, nested `items`, and nested
  `properties`;
- `additionalProperties` schemas and credible `oneOf` or `anyOf` alternatives.

It leaves formats, regular-expression dialects, `multipleOf`, prose-only rules,
and generated `allOf` fragments to Kie. The current catalog contains some
`allOf` fragments whose type contradicts the surrounding object, so enforcing
them would reject valid requests. These fragments remain visible in
`input_schema`; the validator does not guess how to repair them.

`src/kie/catalog/models.rs` remains a small embedded compatibility table. It
stores aliases, prompt policy, simple media bindings, and common convenience
field mappings. The live contract supplies model discovery and validation; the
embedded entry supplies stable request construction. If the docs are offline or
a known route disappears, existing IDs and aliases continue to work without
schema-backed validation.

Review manual changes to the embedded table against the individual route pages.
Preserve exact-key uniqueness and update
`tests/fixtures/kie_catalog_contract.json` only after that review. The snapshot
test locks every embedded request profile. Do not copy full route schemas into
the binary.

Grok Segment Map and the two OmniHuman preparation models live in
`operations.rs`, so a caller cannot send them through `kie_generate_image` or
`kie_generate_video`. Gemini Omni Audio and Character use separate endpoints
and are not generation model entries.

## Configuration reference

| Variable | Default | Purpose |
| --- | --- | --- |
| `KIE_API_KEY` | unset | Required for live API calls. |
| `KIE_MCP_API_BASE` | `https://api.kie.ai` | Kie task and credit API base URL. |
| `KIE_MCP_UPLOAD_BASE` | `https://kieai.redpandaai.co` | Kie upload API base URL. |
| `KIE_MCP_CATALOG_URL` | `https://docs.kie.ai/llms.txt` | Public catalog index; override for tests or a trusted mirror. |
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
cargo run -- debug models --media-type video --query kling --include-descriptions
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
- `kie_task_status` recognizes the three asynchronous preparation models. It
  returns their typed result and downloads masks when `download_if_complete` is
  true.
- Live-only model IDs are accepted when their downloaded route establishes the
  media kind. They require exact model-specific fields in `input` because the
  embedded table does not provide convenience mappings. If no live contract is
  available, raw IDs are accepted only when their media kind can be inferred
  safely. Top-level prompt, media, aspect-ratio, resolution, and output-format
  shortcuts are never guessed for models without an embedded profile.

## Verification

Tests are mock-only and do not require `KIE_API_KEY`. Catalog tests use local
`llms.txt` and route Markdown fixtures, including nested object and array
schemas. They never submit Kie tasks.

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build --release
```

The CI workflow runs formatting, strict Clippy, and the test suite on stable
Rust. Keep tests focused on public request behavior and observed Kie response
shapes rather than internal call structure.
