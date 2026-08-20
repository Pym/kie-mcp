# kie-mcp

A small MCP server that gives your agent access to Kie.ai image and video
generation. It discovers models, uploads local reference media, waits for Kie,
downloads the result, and returns local paths with Markdown previews. Dedicated
tools also cover the preparation steps used by Grok Image 2, Gemini Omni Video,
and OmniHuman 1.5.

## Install

You need a [Kie.ai](https://kie.ai/) API key and Rust 1.85 or newer.

```bash
rustup update stable
cargo install --git https://github.com/Pym/kie-mcp
command -v kie-mcp
```

Use the absolute path printed by `command -v` in your MCP client configuration.
The server uses stdio and starts with `kie-mcp serve` (or simply `kie-mcp`).

## Configure

At minimum, pass `KIE_API_KEY` to the server process. Setting an absolute output
directory is recommended because desktop clients do not always start servers
from the directory you expect.

### Codex

```toml
[mcp_servers.kie]
command = "/absolute/path/to/kie-mcp"
args = ["serve"]
env_vars = ["KIE_API_KEY", "KIE_MCP_OUTPUT_DIR"]
tool_timeout_sec = 1800
```

Set `KIE_API_KEY` and, optionally, `KIE_MCP_OUTPUT_DIR` in the environment that
launches Codex.

### Claude and JSON clients

```json
{
  "mcpServers": {
    "kie": {
      "command": "/absolute/path/to/kie-mcp",
      "args": ["serve"],
      "env": {
        "KIE_API_KEY": "<your-private-key>",
        "KIE_MCP_OUTPUT_DIR": "/absolute/path/to/kie-output"
      }
    }
  }
}
```

Configuration filenames and environment forwarding differ between clients, but
the contract is always the same: an executable, `serve`, environment variables,
and stdio transport.

## Use

Once connected, ask your agent normally:

- "Generate a 16:9 editorial photo with Kie and save it as `cover`."
- "Turn `/absolute/path/product.png` into a five-second product video."
- "Show me the available Nano Banana models."
- "Show me the editable segments in this Grok image."
- "Create a reusable Gemini Omni character from this portrait."
- "How many Kie credits do I have left?"

The server exposes 11 tools. Six cover generation and account operations, while
five handle model-specific preparation:

| Tool | Purpose |
| --- | --- |
| `kie_generate_image` | Generate or edit an image and download it locally. |
| `kie_generate_video` | Generate a video and download it locally. |
| `kie_models` | Find supported model IDs, aliases, and common inputs. |
| `kie_gemini_omni_create_audio_profile` | Create an audio profile ID for Gemini Omni Video. |
| `kie_gemini_omni_create_character` | Create a character ID for Gemini Omni Video. |
| `kie_grok_image_2_segment_map` | Find Grok Image 2 segments and download their masks. |
| `kie_omnihuman_human_identification` | Run OmniHuman's portrait identification preflight. |
| `kie_omnihuman_subject_detection` | Find OmniHuman subjects and download their masks. |
| `kie_task_status` | Check or download an existing task. |
| `kie_upload_media` | Upload a local file for model-specific workflows. |
| `kie_credits` | Read the current account credit balance. |

Generation tools accept a model, a model-aware optional `prompt`, and convenience
fields such as `aspect_ratio`, `resolution`, and `output_format`. `kie_models`
reports whether each model requires, optionally accepts, or does not use a
prompt. A legacy prompt supplied to a promptless model is not forwarded to Kie.
Model-specific Kie fields go in `input`.

For reference media, pass public URLs through `input_urls` or local image/video
files through `local_input_paths`. Local files are uploaded automatically. These
shortcuts require a cataloged media binding; for an uncataloged raw model ID,
put every model-specific field directly in `input` so the server does not guess
Kie's field names.

### Model-specific preparation

The five preparation tools are composable. The server does not call them
silently.

- Gemini Omni Audio returns an `audio_id`. Pass it to Gemini Omni Character or
  to `gemini-omni-video` through `input.audio_ids`.
- Gemini Omni Character returns a `character_id`. Pass it to
  `gemini-omni-video` through `input.character_ids`.
- Grok Segment Map returns a source `task_id` and numbered masks. Generate the
  edit with `grok-imagine-image-2-0/image-edit`, then place the selected indexes
  in `input.mask_indexs`. The field spelling follows Kie's API.
- OmniHuman Subject Detection returns mask URLs and local previews. Pass the
  chosen URLs to `omnihuman-1-5` through `input.mask_url`.
- OmniHuman Human Identification returns Kie's integer `subject_status` as-is.
  Kie's documentation does not define the values, so the server does not label
  them as success or failure.

## Files and settings

Results are downloaded to `output/kie` by default. Each task gets its own
directory, and successful calls return absolute paths that the agent can reuse.

| Variable | Default | Purpose |
| --- | --- | --- |
| `KIE_API_KEY` | unset | Required Kie API key. |
| `KIE_MCP_OUTPUT_DIR` | `output/kie` | Directory for downloaded results. |
| `KIE_MCP_TIMEOUT_SECS` | `900` | Maximum generation wait time. |
| `KIE_MCP_MAX_UPLOAD_BYTES` | `536870912` | Maximum local upload size. |
| `KIE_MCP_INPUT_ROOTS` | unset | Optional allowlist of local upload directories. |

Advanced endpoints, HTTP timeouts, debug commands, architecture, and known
limitations are documented in [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

## Cost and privacy

- Generation and preparation calls can consume Kie credits.
- Prompts, model inputs, URLs, and uploaded files are sent to Kie.
- Generated files remain on disk until you remove them.
- Keep API keys, signed URLs, and sensitive output directories private.

## Development

Tests use a local mock API and do not consume Kie credits. The concurrency regression test launches three simultaneous mocked generations with the same `output_name` and verifies distinct task IDs, downloads, paths, and contents.

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for maintainer notes.

## License

MIT
