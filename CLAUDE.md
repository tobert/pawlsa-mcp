# pawlsa-mcp

Stdio MCP server exposing Linux audio system state (ALSA + PipeWire) as resources and tools.

## Build

Requires `libasound2-dev` and `libpipewire-0.3-dev` (or Arch equivalents `alsa-lib` and `pipewire`).

```
cargo build
```

## Architecture

- **PW thread**: `std::thread` running `pipewire::MainLoop`. Updates `Arc<RwLock<PwState>>` on registry events. Commands from tokio arrive via `pipewire::channel`.
- **ALSA**: Synchronous calls inline in async handlers (fast enough, no spawn_blocking).
- **State bridge**: `std::sync::RwLock` (not tokio's) because the PW writer is a std thread. Tokio side holds read lock only briefly — never across `.await`.
- **Output format**: Columnar text with `, ` column separator and ` ▌ ` row separator. These Unicode characters pass through JSON strings without escaping (unlike `\n`/`\t`). Empty fields render as `N/A`.

## Key files

- `src/main.rs` — tokio entrypoint, tracing to stderr, PW spawn, stdio serve
- `src/server.rs` — `PawlsaServer`, `ServerHandler` impl, URI routing, tool dispatch
- `src/format.rs` — `Table` builder for columnar output
- `src/alsa/` — cards, devices, mixer (read + set), midi
- `src/pw/mod.rs` — PW thread, registry callbacks, command channel handler
- `src/pw/state.rs` — snapshot types, format methods

## MCP surface

Resources: `pawlsa://alsa/cards`, `pawlsa://alsa/midi/ports`, `pawlsa://pw/nodes`, `pawlsa://pw/ports`, `pawlsa://pw/links`

Templates: `pawlsa://alsa/cards/{index}`, `pawlsa://alsa/devices/{category}`, `pawlsa://alsa/mixer/{card_index}`, `pawlsa://pw/nodes/{id}`

Tools: `pw_link_create`, `pw_link_destroy`, `mixer_set_volume`, `mixer_set_switch`

Future: `pw_set_node_props` — requires binding the PipeWire metadata interface, which pipewire-rs doesn't wrap ergonomically yet.

## Testing

```
npx @modelcontextprotocol/inspector -- cargo run
```

Or add to Claude Code MCP config and use `/mcp` to connect.
