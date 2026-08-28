# aios

A daemon that lets coding agents do real work across all of your projects instead of one directory at a time.

Claude Code and Codex are good at editing a repo you point them at. They have no idea what else you own, they each invent their own notion of a task list, and when you close the terminal the run is gone. aios is the layer underneath that. It knows which projects exist, it owns one implementation of the tools those projects share, and it supervises agent runs so they outlive the shell that started them.

It is not an agent. It is the thing agents run on.

**Status: early.** Tier 0 is being built. It works for me, it is not packaged for anyone else yet, and the API will move.

## What it actually does

**Project registry.** One list of your projects with their paths, capabilities and config, so an agent can be told "work on X" rather than being handed a directory and no context.

**One implementation of the shared tools.** Issues, knowledge base and version control are defined once as capabilities and bound to concrete backends in a single composition root. Issues go through [beads](https://github.com/gastownhall/beads), knowledge through an Obsidian vault, VCS through git. Swapping beads for Linear is one file plus one crate, not a rewrite.

**Run supervision.** Agent runs get spawned, streamed, persisted and resumed. A run that takes forty minutes does not die because you closed the laptop lid.

**Three ways in.** The same binary is a CLI, a REST API and an MCP server. Every capability lands in the binary first and is usable from a terminal before any client renders it. If you cannot do it with `aios` in a terminal, it is not done.

## Layout

Twelve crates, about 9,000 lines of Rust.

| crate | what it is |
| --- | --- |
| `aios-types` | The single definition of every wire type. OpenAPI, Swift and TS are generated from it. |
| `aios-core` | Registry, config, the composition root. |
| `aios-caps` | Capability traits. What a backend has to implement. |
| `aios-runs` | Run supervisor. Spawn, stream, persist, resume. |
| `aios-api` | REST server (axum). |
| `aios-cli` | The command line client. |
| `aios-mcp` | MCP server, so harnesses can call aios back. |
| `aios-claude`, `aios-codex` | Harness adapters. |
| `aios-beads`, `aios-obsidian`, `aios-git` | Backend adapters for issues, knowledge and VCS. |

Storage is JSON documents and JSONL append logs under `~/.aios`, no SQLite. The stored form is the same serde type as the wire form, so there is no row-mapping layer to drift. It also means you can read and edit your own state in a text editor, which matters when the thing writing it is a language model.

## Running it

```
just build          # cargo build
just check          # fmt, clippy, tests
cargo run -- doctor # check the local setup
```

Commands:

```
aios project   add / list / show / edit / refresh / remove
aios issue     list / ready / show / new / close / status
aios kb        list / search / read / write / capture
aios vcs       status / log
aios run       start / list / show / resume / events
aios approval  list / show / approve / deny / policy / gate
aios cap       list / schema / call
aios mcp       serve / install / uninstall
aios daemon    install / uninstall / start / stop / status / logs
aios serve     run as the daemon in the foreground
aios doctor    check the local setup
aios version
```

## Design notes

The decisions and the reasoning behind them are in [docs/plan.md](docs/plan.md). Short version:

Rust because it is one static binary with system access, not because it is fast. The daemon spends its life waiting on model calls.

No relay server. Device to device over mDNS on the LAN, Tailscale off it. Nothing we run sits between the daemon and a client, and nothing anyone else runs can read the traffic either.

No push notifications, which means no custody of push tokens or an APNs key, and approvals have to be driven by policy rather than by interrupting you. That is better anyway.

One monorepo, because the OpenAPI spec is a seam inside it. A capability change and every client that consumes it land in the same commit.

## License

MIT.
