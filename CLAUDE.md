# Project Instructions for AI Agents

This file provides instructions and context for AI coding agents working on this project.

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:6cd5cc61 -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/SYNC_CONCEPTS.md for details and anti-patterns.

## Agent Context Profiles

The managed Beads block is task-tracking guidance, not permission to override repository, user, or orchestrator instructions.

- **Conservative (default)**: Use `bd` for task tracking. Do not run git commits, git pushes, or Dolt remote sync unless explicitly asked. At handoff, report changed files, validation, and suggested next commands.
- **Minimal**: Keep tool instruction files as pointers to `bd prime`; use the same conservative git policy unless active instructions say otherwise.
- **Team-maintainer**: Only when the repository explicitly opts in, agents may close beads, run quality gates, commit, and push as part of session close. A current "do not commit" or "do not push" instruction still wins.

## Session Completion

This protocol applies when ending a Beads implementation workflow. It is subordinate to explicit user, repository, and orchestrator instructions.

1. **File issues for remaining work** - Create beads for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **Handle git/sync by active profile**:
   ```bash
   # Conservative/minimal/default: report status and proposed commands; wait for approval.
   git status

   # Team-maintainer opt-in only, unless current instructions forbid it:
   git pull --rebase
   git push
   git status
   ```
5. **Hand off** - Summarize changes, validation, issue status, and any blocked sync/commit/push step

**Critical rules:**
- Explicit user or orchestrator instructions override this Beads block.
- Do not commit or push without clear authority from the active profile or the current user request.
- If a required sync or push is blocked, stop and report the exact command and error.
<!-- END BEADS INTEGRATION -->


## Build & Test

_Add your build and test commands here_

```bash
# Example:
# npm install
# npm test
```

## Architecture Overview

## Architecture

**Read `docs/plan.md` before making structural decisions.** It carries the locked
decisions (§0) and the reasoning behind them. Do not re-litigate a numbered
decision without saying so explicitly.

AIOS is a Rust daemon that runs coding harnesses (Claude Code, Codex) against a
registry of projects, unifying issue tracking (beads), knowledge (Obsidian vault),
and VCS behind one capability layer projected onto REST, MCP, CLI, and a generated
OpenAPI spec.

This is a **polyglot monorepo**: `crates/` (Rust core + the `aios` binary),
`clients/apple/` (SwiftUI macOS + iOS over a shared `AIOSKit` package),
`clients/ts/` (generated TypeScript client).

## Work order — the tier rule

Priority is always, and per feature:

```
Tier 0  the aios binary       ->  Tier 1  local desktop client  ->  Tier 2  mobile client
        core + CLI + API + MCP        (macOS now)                       (iOS now)
```

- A capability lands in the binary first, usable from the CLI and exposed over REST
  and MCP. Only then does the desktop client render it, and only then does mobile
  take the subset that makes sense away from a desk. Never the reverse.
- **The test:** can you do it with `aios` in a terminal? If not, the core is not
  done and no client work should start.
- **Clients contain presentation only.** No domain logic in view models. If a client
  needs to know a rule, the API is missing an endpoint.
- macOS and iOS are the current *implementations* of two client roles, not the roles
  themselves. Nothing in the core may assume Apple.

## Conventions

- Nothing but `aios daemon *` subcommands may touch `aios-core` directly; every
  other surface is an API client.
- The `IssueTracker` port goes through the `bd` CLI. Never read `.beads/` directly.
- `openapi.json` is generated and committed; it is the contract for all clients.

## Types and API compatibility

**Every type that crosses a boundary is defined once, in `crates/aios-types`, and
nowhere else.** OpenAPI, Swift and TypeScript models are all derived from it. See
`docs/plan.md` §15.

- Derive `Serialize, Deserialize, ToSchema` on every wire type. `#[utoipa::path]`
  will not compile without `ToSchema`, so a type cannot reach the API without
  entering the derivation chain.
- `#[serde(rename_all = "camelCase")]` everywhere.
- Enums are **internally tagged**: `#[serde(tag = "type", rename_all = "camelCase")]`.
  Untagged and externally-tagged enums generate poor or wrong Swift.
- Newtype ids (`ProjectId`, `RunId`), never bare `String`. No `serde_json::Value`
  in wire types.
- After changing any wire type run `just openapi`. The committed `openapi.json`
  being stale is a CI and pre-commit failure.
- Changing the spec requires bumping `apiVersion`; a *breaking* change also raises
  `minClientApi`. CI classifies which via `oasdiff` and fails if the bump is missing.

## Capabilities

A capability is registered once in `crates/aios-caps/src/caps/` and is thereby
callable from the CLI, MCP, and REST. Adding one means:

1. Put its input/output types in `aios-types` (never inline in the handler).
2. Register it in the relevant `caps/*.rs` `register()` function with a
   `group.operation` name, a summary, and the correct `Effect`.
3. `Effect::Write` is not decoration: it drives MCP annotations, read-only agent
   profiles, and (from phase 3) whether a call requires an approval. A
   misclassification is a security bug.

Handlers must go through a port trait (`IssueTracker`, `Knowledge`, `Vcs`), never
call a tool directly. The composition root that binds ports to concrete adapters
is `crates/aios-cli/src/app.rs` -- the only place that names beads, Obsidian, or
git.

CLI commands call `Capabilities::call(...)` by name rather than the ports, so the
CLI exercises the same path MCP and REST will take.
