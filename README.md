# kie-mcp

A small MCP server that gives your agent access to Kie.ai image and video
generation. It discovers models, uploads local reference media, waits for Kie,
downloads the result, and returns local paths with Markdown previews.

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
- "How many Kie credits do I have left?"

The server exposes six tools:

| Tool | Purpose |
| --- | --- |
| `kie_generate_image` | Generate or edit an image and download it locally. |
| `kie_generate_video` | Generate a video and download it locally. |
| `kie_models` | Find supported model IDs, aliases, and common inputs. |
| `kie_task_status` | Check or download an existing task. |
| `kie_upload_media` | Upload a local file for model-specific workflows. |
| `kie_credits` | Read the current account credit balance. |

Generation tools accept a model, a prompt, and optional convenience fields such
as `aspect_ratio`, `resolution`, and `output_format`. Model-specific Kie fields
go in `input`. The tool schemas tell the agent which fields are available; use
`kie_models` when a model name or media binding is unclear.

For reference media, pass public URLs through `input_urls` or local image/video
files through `local_input_paths`. Local files are uploaded automatically.

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

- Generations and parallel variants can consume Kie credits.
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
