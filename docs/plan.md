# AIOS — Architecture Plan

> An operating layer that lets AI drive real work through conventional coding
> harnesses (Claude Code, Codex), against a registry of known projects, with a
> unified set of backing tools (issues, knowledge, VCS) and pluggable
> conversation channels (admin chat, WhatsApp, apps).

---

## 0. Decisions locked (2026-08-24)

| # | Decision | Choice | Rationale |
| - | -------- | ------ | --------- |
| 1 | Core language | **Rust** — one static binary, `axum` + `tokio` + `rusqlite` | Supersedes an earlier TypeScript decision. The single-binary + system-access requirement (§3.1) is native in Rust and awkward in Node; Rust + Swift is a coherent two-language story where TS in the middle would have been a third language destined for rewrite. Performance is *not* the reason — the daemon is I/O-bound on model calls. |
| 2 | Issue storage | **Per-project `.beads/`, with the JSONL export committed** | Harnesses see issues natively in-repo; worktrees stay coherent. Cross-project view via `bd repo` + federation. Note the correction in §6.1: the Dolt DB itself is gitignored by beads, so `issues.jsonl` is what actually travels with the repo. |
| 3 | Vault | **`~/vault`, its own git repo** | Independent history, syncs on its own, swappable via config, openable in Obsidian without code noise. |
| 4 | Channels | **Deferred** | Nothing in phases 0–3 depends on it. Revisit after the run supervisor is real. See §8 for the investigation already done. |
| 5 | Remote client | **Native SwiftUI app**, types generated from the daemon's OpenAPI spec | Platform integration matters for a long-running-agent monitor; generation removes the schema-drift cost that would otherwise argue against native. §13.5–13.6 |
| 6 | Transport | **Direct device-to-device** — mDNS on LAN, Tailscale P2P off-LAN. No relay. | No server we run sits between daemon and app, and none that anyone runs can read the traffic. §13.1 |
| 7 | Push | **None** | No custody of push tokens or an APNs key until the app warrants securing them. Removes the last third party from the data path; forces approvals to be policy-driven rather than interrupt-driven, which is better anyway. §13.3 |
| 8 | Process model | **One binary, three modes** — `aios serve` (daemon), `aios <cmd>` (CLI client), managed LaunchAgent | Same executable however it is started; the Mac app installs and supervises it, but never owns it. §3.1 |
| 10 | Repo shape | **One polyglot monorepo** — Rust crates, Apple apps, TS client, docs, issues | The OpenAPI spec is a seam *inside* the repo, so a capability change and every client that consumes it land in one commit. Split repos would make each schema change a multi-repo dance with version pinning. §11.1 |
| 9 | Client tiers | **Binary → desktop → mobile**, always and per feature. macOS/iOS are the first implementations of the two client roles. Next.js web UI dropped. | Clients hold presentation only; the CLI being complete is what keeps them honest. Two SwiftUI targets over one shared `AIOSKit` also costs far less than a React admin *and* a SwiftUI admin. §1.1, §14 |


---

## 1. The core idea in one paragraph

AIOS is **not** an agent. It is the substrate agents run on. It owns three things
that no individual coding harness owns: (a) *which projects exist and what they
are*, (b) *a single canonical implementation of the tools those projects share*
— issue tracking, knowledge base, VCS, and (c) *the lifecycle of agent runs* —
spawning, streaming, persisting, resuming. Harnesses become interchangeable
execution backends. Channels become interchangeable front ends. What stays fixed
in the middle is the registry, the capabilities, and the run supervisor.

### 1.1 Work order — the tier rule

**Standing priority, not just an initial build order:**

```
Tier 0   the aios binary          core + CLI + API + MCP
   ↓
Tier 1   local desktop client     macOS SwiftUI now; the role is "full admin,
                                  same machine, over the Unix socket"
   ↓
Tier 2   mobile client            iOS now; the role is "on-the-go triage and
                                  conversation, over the network"
```

macOS and iOS are the *current implementations* of two roles, not the roles
themselves. A Linux or Windows desktop client is a Tier 1 client; an Android app is
a Tier 2 client. Nothing in the core may assume Apple.

**The rule applies per feature, not just to the initial build.** Every capability
lands in the binary first — usable from the CLI, exposed over REST and MCP — then
the desktop client renders it, then mobile takes whatever subset makes sense away
from a desk. Never the other way round. When channels arrive they follow the same
path; so does everything after them.

**The test is concrete:** *can you do it with `aios` in a terminal?* If not, the
core is not done and no client work should start. This is checkable rather than
aspirational, which is the point.

**What this guards against.** The usual way a system like this rots is that the
desktop app quietly accumulates domain logic in its view models — approval rules,
run-state derivation, project resolution — which mobile then cannot reuse, the CLI
never had, and the API cannot express. So:

> **Clients contain presentation only.** `AIOSKit` may hold client-side *state
> management* — caching, the outbox, stream resumption. It may not hold domain
> logic. If a client needs to know a rule, the API is missing an endpoint.

That is the same constraint as the API-only rule in §3.1, stated as a work-order
principle: the CLI being a complete interface is what keeps every other client
honest.

**Contracts, so a future client is not a redesign:**

| Tier | Must do | Must not do |
| ---- | ------- | ----------- |
| 1 — desktop | Connect over UDS; manage daemon lifecycle; full admin surface; run inspection with diffs; approval triage; KB browse; persistent tray/menu-bar presence | Hold domain logic; be required by the core |
| 2 — mobile | Network transport with pairing; triage-first UI; conversation; resumable streams; offline outbox | Manage daemon lifecycle; assume it is the only client |

## 2. The one design decision everything else hangs off

**Capabilities are defined once and projected onto four surfaces.**

A capability is a named operation with `serde` input/output types and an
implementation bound to a port trait (`IssueTracker`, `Knowledge`, `VCS`, ...).
From that single definition AIOS derives:

1. a **typed Rust function** (used internally by every other surface),
2. a **REST route** on the daemon (used by the apps and anything you build),
3. an **MCP tool** (used by Claude Code, Codex, Claude.ai, anything MCP),
4. a **CLI subcommand** + a **skill doc** (used by humans and by harnesses that
   prefer shelling out),
5. an **OpenAPI schema entry**, from which the Swift and TypeScript clients are
   generated (§13.6).

Rust makes this cleaner than the original TS design: the `serde` structs *are* the
schema, `utoipa` derives the OpenAPI document from the same types and handlers,
and `clap` derives the CLI from them too. There is no second schema language to
keep in sync.

```
                    ┌─────────────────────┐
                    │  capability def     │  name + serde types + impl
                    └──────────┬──────────┘
        ┌────────┬─────────┴──┬──────────┬─────────┐
        ▼        ▼            ▼          ▼         ▼
      Rust     REST/SSE      MCP     CLI+skill  OpenAPI
      core                                       └─▶ Swift + TS clients
```

This is what makes "integrations are translatable as skills and app
integrations" true by construction rather than by discipline. Adding Linear later
means writing one adapter, and Claude, Codex, the web UI, WhatsApp and your apps
all get it simultaneously.

**Ports (interfaces) and their first implementations:**

| Port           | First impl        | Swappable later                 |
| -------------- | ----------------- | ------------------------------- |
| `IssueTracker` | beads (`bd`)      | Linear, GitHub Issues, Jira     |
| `Knowledge`    | Obsidian vault    | any markdown tree, Notion       |
| `VCS`          | git + gh          | —                               |
| `Harness`      | claude, codex     | aider, cursor-agent, custom     |
| `Channel`      | web chat          | WhatsApp, Telegram, Slack, SMS  |

## 3. Components

```
┌──────────────────────────────────────────────────────────────────┐
│              aios  (one static Rust binary, `aios serve`)        │
│  ┌────────────┐ ┌──────────────┐ ┌───────────┐ ┌──────────────┐  │
│  │  Registry  │ │ Capabilities │ │   Runs    │ │  Channels    │  │
│  │ projects,  │ │  issues/kb/  │ │ supervisor│ │  router      │  │
│  │ agents     │ │  vcs/memory  │ │ +approvals│ │  (deferred)  │  │
│  └────────────┘ └──────────────┘ └───────────┘ └──────────────┘  │
│  state: SQLite (~/.aios/state.db)   bus: cursor-addressed events │
│  surfaces:  UDS ~/.aios/aiosd.sock   ·   TCP+TLS (LAN, tailnet)  │
│             MCP (stdio + streamable HTTP)                        │
└──┬────────────┬──────────────┬──────────────┬────────────────────┘
   │ UDS        │ UDS          │ stdio        │ TCP
 aios CLI    macOS app     claude/codex     iOS app
             (SwiftUI)    (as MCP clients)  (SwiftUI, direct P2P)
                └──────── AIOSKit ─────────────┘
                     (shared Swift package)
```

**Why a daemon and not just a CLI:** scheduled runs, inbound channel messages, and
long-lived background agent sessions all need something always-on, and you need
exactly one writer to the run/session state.

### 3.1 One binary, three modes

There is a single executable, `aios`. How it is started decides what it is:

| Invocation | Role |
| ---------- | ---- |
| `aios serve` | The daemon. Foreground, logs to stdout. What launchd and the Mac app actually exec. |
| `aios <command>` | CLI client. Connects over the Unix socket; autostarts the daemon if absent. |
| `aios daemon install\|start\|stop\|status\|logs` | LaunchAgent lifecycle management. The only commands that don't go through the API. |

**Ownership rule: the Mac app manages the daemon but never owns it.** The app
bundles the binary in `Contents/Resources/` and registers it as a **LaunchAgent via
`SMAppService`**, rather than spawning it as a child process. That distinction is
the whole point — a child process dies when you quit the app, and an OS whose
agents stop when you close a window isn't an OS. Registered as a LaunchAgent it
survives app quit, logout, and reboot, and `aios` on the command line talks to the
exact same instance.

Headless installs skip the app entirely: drop the binary on the box, run
`aios daemon install`. Same binary, same LaunchAgent, no GUI dependency anywhere.

**The API-only rule:** nothing but the `aios daemon *` subcommands may touch
`aios-core` directly. The CLI, the Mac app, and the iOS app are all API clients
with no privileged path. This is what keeps the API honest — if a screen needs
something the API can't express, that is a missing endpoint, not a shortcut.

### 3.2 Transports — right channel for each caller

| Caller | Transport | Auth |
| ------ | --------- | ---- |
| CLI, Mac app (same machine) | **Unix domain socket** `~/.aios/aiosd.sock`, mode `0600` | Filesystem permissions. No TCP, no TLS, no token, nothing on the network at all. |
| Other devices on the LAN | TCP + TLS, discoverable via mDNS `_aios._tcp.local` | Self-signed cert whose fingerprint is pinned via the pairing QR; device token on every request. |
| Devices off-LAN | TCP + TLS on the tailnet address | Real Let's Encrypt cert via `tailscale cert`; device token. |
| Harnesses | MCP over stdio (local) or streamable HTTP | Inherited from the spawning run. |

Same `axum` router served over every transport — the handlers never know which one
they were reached through. Same-machine admin never touches the network stack,
which makes the common case both the fastest and the most private.

**Never bind `0.0.0.0`.** LAN exposure is explicit opt-in per interface, and the
device token is required regardless of path: network location is never treated as
authentication.

## 4. Project registration

`aios project add ~/projects/foo` performs:

1. **Detect** — git remote, default branch, language/package manager, existing
   `.beads/`, existing `CLAUDE.md` / `AGENTS.md`.
2. **Record** — row in the registry: slug, path, remote, tags, issue prefix,
   kb path, allowed harnesses, default model, run policy.
3. **Provision issues** — `bd init` inside the project (`.beads/`, committed) and
   register it in the global `bd repo` config so AIOS can query across all
   projects.
4. **Provision knowledge** — create `<vault>/projects/<slug>/` with a seeded
   `index.md`, plus an ignored symlink `<project>/.aios/kb → vault/projects/<slug>`
   so harnesses can read/write KB with plain file tools without the markdown
   entering the project's git history. (Ignore via `.git/info/exclude`, never
   `.gitignore` — that file belongs to the project, not to us.)
5. **Provision harness config** — write `<project>/.mcp.json` (or `.aios/` +
   a managed block in `CLAUDE.md` / `AGENTS.md`) pointing at the AIOS MCP server,
   using **fenced managed regions** so hand edits survive regeneration.

State lives in `~/.aios/`:

```
~/.aios/
  config.toml        vault path, daemon port, defaults
  state.db           projects, sessions, runs, messages, channel identities
  agents/*.md        agent profiles (frontmatter + system prompt)
  runs/<id>/         transcripts, logs, artifacts
  logs/
```

Everything durable that benefits from history (vault, per-project `.beads/`) is
git-tracked in its own repo. `state.db` is ephemeral-ish operational state and is
backed up, not versioned.

## 5. Knowledge model

- **One Obsidian vault**, its own git repo, at `~/vault` (or wherever you point).
- `global/` — cross-project knowledge, your preferences, patterns, decisions.
- `projects/<slug>/` — project-specific notes. Not stored in the project repo.
- `daily/`, `inbox/` — capture surface for channel conversations.
- Retrieval starts as **plain grep + frontmatter/tag filters over markdown**,
  which is fast enough for a personal vault and keeps the store human-editable.
  Add embeddings later behind the same `Knowledge` port only if grep stops
  cutting it — don't build a vector DB on day one.
- Wikilinks `[[...]]` are first-class: the KB capability resolves and traverses
  them, so an agent can follow context the way you do in Obsidian.

## 6. Issue model

Per-project `.beads/` DBs (committed to each project) rather than one global DB.
Rationale: issues travel with the repo, a harness working in the repo sees them
natively with zero AIOS involvement, and branching/worktrees stay coherent. AIOS
adds the cross-cutting view on top via `bd repo` + federation, plus one extra
"personal" beads DB inside the vault repo for non-project and cross-project work.

The `IssueTracker` capability normalizes beads into a neutral shape (id, title,
status, priority, type, deps, labels, project) so Linear can slot in later
without rewriting prompts, skills, or the UI.

### 6.1 Correction — what actually travels with the repo

Verified at setup rather than assumed: **beads gitignores its own database.**
`bd init` writes `.beads/.gitignore`, which excludes `embeddeddolt/` along with
sockets, locks and sync state. The source of truth is a local embedded Dolt
database, and beads' intended cross-machine mechanism is **Dolt remotes**
(`bd dolt push`), not git.

That undercuts the original rationale for decision #2 — "issues travel with the
repo" was not true as configured. Resolution, in keeping with local-first:

- **`export.auto = true`** is enabled, so beads writes `.beads/issues.jsonl` after
  write commands (throttled to ~60s). That file *is* git-trackable and is the thing
  that travels with a clone: diffable in PRs, readable without bd, restorable via
  `bd import`.
- The local Dolt DB remains the source of truth and keeps full history; the JSONL
  is an export, and beads is explicit that it is not a backup.
- **A Dolt remote is now configured, and it rides the git remote.** `bd dolt remote
  add origin git@github.com:silurt/aios.git` + `bd dolt push` works, so there is no
  DoltHub and no third-party host — it does not cut against §13.8. Beads stores the
  database under its own ref namespace, **`refs/dolt/data`**, plus a
  `__dolt_remote_info__` branch; `refs/heads/main` is untouched, so a normal clone
  or a GitHub diff never sees Dolt internals.
- Consequence: **full issue history syncs with the repo**, and the JSONL export is
  demoted to convenience — a human-readable view for diffs and for reading issues
  without `bd`. Beads deletes it entirely when there are no issues, which is why it
  is absent from the tree right now.

Practical consequence for AIOS: the `IssueTracker` port must treat `bd` as the
interface and never read `.beads/` files directly, because the on-disk layout is
beads' business and clearly subject to change.

## 7. Runs and sessions

- **Session** = a durable conversation. Bound to (agent profile, optional
  project, channel, harness). Transcript persisted in `state.db`.
- **Run** = one execution of a harness process inside a session. Has a
  workspace, a status, a streamed event log, and an exit result.
- **Workspace** = either the project checkout directly (interactive, foreground)
  or a **git worktree** (background/parallel), so async agents never disturb what
  you have open. `bd worktree` already understands this pattern.

**Harness adapter interface:**

```rust
#[async_trait]
trait Harness {
    fn id(&self) -> HarnessId;                                  // Claude | Codex
    async fn start(&self, opts: StartOpts) -> Result<RunHandle>; // cwd, prompt, model, mcp
    async fn send(&self, run: &RunHandle, text: &str) -> Result<()>;
    fn events(&self, run: &RunHandle) -> BoxStream<'_, HarnessEvent>;
    async fn interrupt(&self, run: &RunHandle) -> Result<()>;
    async fn resume(&self, session_ref: &str) -> Result<RunHandle>;
}
```

Both harnesses are driven the same way — as subprocesses speaking newline-delimited
JSON: `claude -p --output-format stream-json --input-format stream-json` and
`codex exec --json`. The Claude Agent SDK is TypeScript/Python-only and therefore
out of reach from Rust, which turns out to be a simplification rather than a loss:
one mechanism, one parser shape, no per-harness special case. Both emit into a
single normalized stream (`message`, `tool_use`, `tool_result`, `approval_request`,
`thinking`, `error`, `done`) so no consumer branches on harness type.

### 7.1 Approvals — a core object, not a UI detail

Harnesses block on permission prompts ("Claude wants to run `git push --force`").
Left as an inline prompt owned by the harness process, that prompt is only
answerable by whoever is staring at the terminal — which makes every non-desktop
surface useless for anything but watching.

So AIOS lifts it out: **an approval is a first-class, addressable, persisted,
awaitable object**, and the harness adapter's job is to translate its native prompt
into one.

```rust
struct Approval {
    id: ApprovalId,
    run_id: RunId,
    request: ApprovalRequest,   // tool, summary, detail, scope
    state: ApprovalState,       // Pending | Approved | Denied | Expired
    decided_by: Option<Decider>,// Policy | User
    policy: ApprovalPolicy,     // auto_rule, expires_at, on_expiry: Park
}
```

Three properties do the work:

- **Pre-authorisation rules** per project and per agent profile decide the common
  cases automatically. The best approval is the one never asked, and this is where
  nearly all the leverage is.
- **Expiry parks the run, it does not kill it.** A parked run is resumable from
  exactly the gate it stopped at, so an unanswered approval costs latency, not
  work.
- **Any surface can decide it** — CLI, web UI, iOS app, or a policy — because it is
  a row with an id rather than a prompt on a pty.

This matters more given the no-push decision (§13.3): the daemon cannot rely on
reaching you, so runs must degrade gracefully when nobody answers.

## 8. Channels and agent profiles

A **channel** is anything that carries a conversation:

```ts
interface Channel {
  id: string
  inbound(raw: unknown): NormalizedMessage   // + identity resolution
  send(sessionId: string, msg: OutboundMessage): Promise<void>
}
```

An **agent profile** is a markdown file with frontmatter — the declarative unit
that makes "a WhatsApp agent that behaves like a fresh Claude with a specific
brief" a config change rather than code:

```markdown
---
name: ops
harness: claude
project: null            # or a slug to bind it to one project
channels: [whatsapp:+49..., web]
tools: [issues.*, kb.*, projects.list]
model: claude-opus-5
memory: rolling          # fresh | rolling | pinned
---
You are my ops agent. You triage, capture to the KB, and file issues...
```

Routing: inbound message → resolve identity → resolve agent profile → find or
create session → enqueue run → stream output back through the channel. The
channel layer never knows which harness answered.

### WhatsApp: what is actually possible (investigated 2026-08-24)

Decision deferred, but the research is done so it doesn't need repeating:

- **Desktop app automation — ruled out.** `/Applications/WhatsApp.app` exposes no
  AppleScript dictionary (`NSAppleScriptEnabled` and `OSAScriptingDefinition` are
  both absent from its Info.plist). Only Accessibility-API UI scripting remains:
  breaks on every update, can't reliably read inbound, needs the app focused.
- **Local SQLite store — read-only curiosity.** Lives under
  `~/Library/Group Containers/group.net.whatsapp.WhatsApp.shared/`. Undocumented,
  version-unstable, needs Full Disk Access, and cannot send. Fine for a one-off
  export, not for a live channel.
- **Baileys — the real "local client".** Implements the multi-device protocol in
  Node and pairs by QR as a *linked device*, exactly as WhatsApp Web and Desktop
  do. Same personal number, same chats, one of the 4 linked-device slots, no
  Business account. Unofficial and ToS-violating; ban risk is real but tracks
  spam-shaped behaviour, which a self-only allowlist at human pacing avoids.
  The **"Message Yourself"** thread makes an ideal admin channel — private, already
  exists, nothing visible to other contacts.
- **Cloud API — the safe path.** Official and ban-proof, but needs a Meta Business
  account and a separate number, and can never see or reply in existing personal
  chats. Better as a later swap-in than a starting point.

Whichever wins, it lands behind the `Channel` port, so the router and agent
profiles never learn which one is in play.

**Note (superseded by §13.1):** an earlier draft put a stateless relay on Vercel in
front of the daemon to hold a stable public URL. The local-first decision removes
it. Only a *webhook-based* channel would need one; a linked-device transport dials
outbound and does not. Treat "requires a public endpoint" as a real cost when the
channel decision is revisited.

## 9. Security posture

This thing can run arbitrary coding agents against all your repos and answers to
a phone number, so:

- Daemon binds loopback plus the tailnet interface only — never `0.0.0.0`. TLS on
  the tailnet address via `tailscale cert`; the LAN/mDNS path is additionally gated
  by the device token.
- Device tokens are per-device, scoped, revocable (`aios device revoke`), and
  carried on every request regardless of which network path reached the daemon —
  network location is never treated as authentication.
- Channel identity allowlist — unknown WhatsApp numbers are dropped, not
  answered.
- Per-agent-profile tool allowlists; destructive capabilities (push, force
  operations, deletes) are opt-in per profile and per project.
- Every run is journaled with its full tool trace (`bd audit` already does
  something like this for issue work).
- No secrets in the vault or in prompts; `~/.aios/config.toml` is `0600`.

## 10. Build phases

| Phase | Deliverable | Proves |
| ----- | ----------- | ------ |
| 0 | Cargo workspace, `aios` binary, SQLite registry, `project add/list/show` | the registry shape |
| 1 | capability registry + port traits + beads/obsidian/git adapters, CLI-exposed | ports & schemas hold up |
| 2 | MCP server (stdio) + per-project config generation | claude *and* codex see identical tools |
| 3 | run supervisor + **approvals with policy** (§7.1), normalized events, transcripts | the harness abstraction, and graceful degradation when nobody answers |
| 4 | `aios serve` — axum over UDS, cursor-addressed SSE, device pairing, `aios daemon install` | the always-on layer |
| 4.5 | `utoipa` OpenAPI spec, committed + staleness check; generated Swift/TS clients | clients can be built without hand-written models |
| 5 | **macOS admin app** — projects, runs, approvals, KB, menu bar extra, daemon lifecycle | the visual admin (§14) |
| 6 | TLS/TCP + mDNS + Tailscale; **iOS app** — pairing, inbox, chat, runs, approval triage | remote control (§13) |
| 7 | agent profiles + channel router + a first channel | agents that reach you where you already are |
| 8 | schedules, cross-project queries, hosted MCP | the "OS" claim |

Two reorderings versus the earlier draft, both because the Mac app changed the
shape: the macOS admin now lands **before** the iOS app (it needs no TLS, no
pairing, no mDNS — just the Unix socket, so it is dramatically cheaper and gives
you a usable product sooner), and channels moved after both clients since nothing
else depends on them.

Phases 0–3 remain the load-bearing ones and are usable from the CLI alone; if the
project stopped at 3 it would still be worth having.

## 11. Repo layout

A Cargo workspace for the core, an Xcode workspace for the clients, and a thin
generated seam between them.

```
aios/
  crates/
    aios-cli/          # the binary: `aios` — serve | client | daemon lifecycle
    aios-core/         # registry, state, config, event bus, types
    aios-caps/         # capability registry, port traits, serde schemas
    aios-runs/         # run supervisor, harness adapters, approvals
    aios-api/          # axum router, served over UDS + TLS/TCP; utoipa spec
    aios-mcp/          # MCP server built from the capability registry
    aios-beads/        # IssueTracker impl
    aios-obsidian/     # Knowledge impl
    aios-git/          # VCS impl
    aios-channels/     # deferred
  clients/             # every client lives here, one dir per platform
    apple/
      AIOSKit/         # shared SwiftPM package: generated client, models,
                       #   @Observable stores, SSE handling, keychain, outbox
      AIOS-macOS/      # tier 1 — SwiftUI admin app + menu bar extra
      AIOS-iOS/        # tier 2 — SwiftUI triage app
    ts/                # generated TypeScript client for your own apps
                       #   (and the seed of any future web or Linux client)
  openapi.json         # generated, committed, the contract between the two worlds
  docs/
```

**Core stack:** `tokio`, `axum` (UDS and TLS from one router), `rusqlite` +
`refinery` for state and migrations, `serde` throughout, `utoipa` for the OpenAPI
document, `clap` (derive) for the CLI, `rmcp` for MCP, `tracing` for logs,
`mdns-sd` for LAN advertisement, `portable-pty` only if an interactive TTY is
genuinely needed — headless harness modes are JSON over pipes and need none.

**Apple stack:** SwiftUI, `@Observable`, SwiftData for cache and outbox,
`swift-openapi-generator` as a build plugin over `openapi.json`.

**Deliberately not here:** no Next.js app. The macOS app is the admin surface
(§14). A browser UI can be added later off the same OpenAPI spec if you ever want
one, and the generated TS client in `clients/ts/` keeps that door open along with
any app you build.

### 11.1 Monorepo mechanics

**This is one repository**, explicitly — three toolchains, one history, one version.

**Why it is the right call here:** `openapi.json` is a seam *inside* the repo. A
capability change regenerates the spec, the Swift client, and the TS client, and all
of it lands in a single commit that either compiles or doesn't. Split repos would
turn every schema change into a multi-repo dance with version pinning and a window
where the clients are wrong — which is precisely the drift §13.6 exists to prevent.

**Three build systems, one entry point:**

| Area | Build system | Root artifact |
| ---- | ------------ | ------------- |
| `crates/**` | Cargo workspace (`[workspace] members = ["crates/*"]`) | `aios` binary |
| `clients/apple/**` | Xcode workspace + SwiftPM (`AIOSKit` as a local package) | `AIOS.app`, `AIOS-iOS.app` |
| `clients/ts/**` | npm | generated TS client |

A root **`justfile`** is the single entry point — `just build`, `just check`,
`just openapi`, `just mac` — delegating to `cargo`, `xcodebuild`, and `npm`. It is
the only tool that spans all three, and it needs no bootstrap beyond `brew install
just`. Codegen that must run in Rust context (emitting the spec from `utoipa`)
stays a `cargo xtask` invoked by the justfile, so it needs nothing installed at all.

**Build ordering — the one real coupling.** The Mac app bundles the daemon binary
in `Contents/Resources/`, so the Xcode build depends on the Cargo build. A run-script
build phase does `cargo build --release` and copies the result in. Target arm64 only
unless Intel support is ever wanted; `lipo` is available if so. Keep this
unidirectional: **Rust never depends on Swift.**

**One version for everything.** Tag at the repo root; app version, daemon version,
and spec version are the same number by construction. This makes the version-skew
handling in §14.3 nearly free — mismatch means someone is running a stale build, not
that two independently-versioned components disagree.

**CI is path-filtered:** Rust jobs on `crates/**`, Swift jobs on `clients/apple/**` (macOS
runner, the expensive one), and a spec-staleness check that always runs because it
is the seam that matters.

**Issues stay in one beads DB** (prefix `aios`) for the whole monorepo, partitioned
by label — `area:core`, `area:mac`, `area:ios`, `area:mcp` — rather than by separate
databases. Cross-area dependencies are the common case here ("iOS approval triage
blocked on approvals API"), and beads' dependency graph only works within a DB.

**Dogfooding:** once `aios project add` exists, this repo is its own first entry.

## 12. Deliberate non-goals (for now)

- No custom agent loop or model calls of our own — harnesses do that.
- No vector database.
- No multi-user / multi-tenant. Single operator, single machine.
- No servers we operate between the daemon and its clients — direct connections only.
- No push, no push tokens, no APNs key (§13.3).
- No telemetry, analytics, or crash reporting.
- No replacing Obsidian or beads UIs — AIOS writes what they already read.

## 13. Tier 2: the mobile client (iOS first)

### The framing that keeps this cheap

The app is **not a special case**. It is one more consumer of the same API the CLI
and the macOS app use, over the same generated client — and by the time it is built
it shares `AIOSKit` with the Mac app (§14), so most of its networking and state
layer already exists and is proven. Combined with the API-only rule from §3.1, the
iOS app costs transport, pairing, and UI work — nothing else.

Only three things are genuinely mobile-specific engineering — reachability,
resumable streams, and pairing. Everything else is screens.

### 13.1 Reachability — direct device-to-device, no server in the path

**Principle: no intermediate server.** The daemon and the phone talk directly,
including over cellular. Nothing we run sits between them, and nothing anyone else
runs can read what passes.

**Data plane: Tailscale (WireGuard).** Mac and iPhone both join the tailnet; the app
connects to `https://mac.<tailnet>.ts.net:7777`. In the normal case this is a
**direct peer-to-peer UDP connection** established by NAT hole-punching — the
packets go phone → Mac with no server in the path at all. Specifically:

- Tailscale's coordination server brokers *public keys only*. Private keys never
  leave either device, and it is structurally unable to decrypt traffic.
- DERP relays exist only as fallback when hole-punching fails (symmetric NAT,
  hostile CGNAT). Traffic through them stays end-to-end WireGuard-encrypted — the
  relay forwards ciphertext it cannot read.
- `tailscale cert` + MagicDNS issues a real Let's Encrypt cert for the machine
  name, so iOS App Transport Security is satisfied with **zero ATS exceptions**.

So: no server in the data path in the common case, and never a server that can
read anything. If even the coordination SaaS is unacceptable, **Headscale** is a
drop-in open-source control plane — but it needs a publicly reachable host, which
reintroduces exactly the server we were removing. For a personal system that trade
goes the wrong way; revisit only if the free tier's terms ever change.

**LAN path: Bonjour, zero third parties.** At home the app should not leave the
local network at all. The daemon advertises `_aios._tcp.local` via mDNS; the app
races candidates on launch and picks the first that answers:

```
1. mDNS-discovered LAN address     ← home wifi: nothing external involved
2. https://mac.<tailnet>.ts.net    ← elsewhere: direct P2P via WireGuard
```

One ordered list, no branching in feature code.

**Alternatives considered and rejected:**

| Approach | Why not |
| -------- | ------- |
| Plain self-configured WireGuard | Truly zero third party, but needs one side to have a stable reachable endpoint. A home Mac behind CGNAT has none, and dynamic prefixes break it. |
| Direct IPv6, no NAT | Genuinely serverless where both ends have IPv6 — but depends on ISP prefix stability, home firewall rules, and carrier IPv6. Tailscale already exploits this path automatically when available, without the fragility. |
| Hand-rolled hole-punching, APNs as the signalling channel | Neat idea — a silent push carries the Mac's current endpoint, the app dials it directly. But it still needs STUN to learn that endpoint, still dies under CGNAT, and means hand-rolling NAT traversal *and* a secure transport. WireGuard already solved both. |
| Vercel / Cloudflare relay | **Dropped from the plan entirely.** It was only ever there for webhook-based channels, which are deferred. |

**Consequence for channels:** the local-first rule quietly settles a later
question. A webhook channel (WhatsApp Cloud API) *requires* a public endpoint and
therefore a relay; a linked-device transport (Baileys, §8) dials outbound and
needs none. So when channels come back, the local-first principle argues for the
linked-device route on its own merits.

### 13.2 Resumable event streams — the non-obvious requirement

Mobile connections drop constantly: backgrounding, cell handoff, elevator. If the
run event stream is a plain WebSocket firehose, every drop loses state and the app
shows a stale or empty run.

So the daemon's event bus must be **cursor-addressable from the start**: every
event gets a monotonic per-session id, the daemon retains a bounded window, and
clients reconnect with `?since=<eventId>` to replay the gap. SSE with
`Last-Event-ID` gives this almost for free and survives mobile networks better
than raw WS.

This is a **Phase 4 architectural requirement, not a mobile feature** — retrofitting
a cursor onto a fire-and-forget bus later means touching every producer.

Symmetrically, the app needs an **outbox**: messages composed offline queue
locally and flush on reconnect, with idempotency keys so a double-flush doesn't
double-send.

### 13.3 Notifications — **no push (decided)**

**Decision: no APNs, no push, no Apple Developer Program dependency.** We do not
want custody of push tokens or an APNs signing key before the app is established
enough to justify securing them. The payoff is that the last third party leaves
the data path: with push gone, **no external service sees anything at all** — not
metadata, not timing, not an event count.

That costs the lock-screen approval flow, which was the strongest argument for the
app. So it has to be compensated for in the daemon rather than papered over.

**What replaces it, in order of reliability:**

1. **App open = fully live.** SSE over the direct connection (§13.1, §13.2). While
   you are looking at it, everything is real-time — approvals, run output, agent
   messages. This is the primary and only *guaranteed* path.
2. **Background refresh → local notifications.** `BGAppRefreshTask` wakes the app
   opportunistically, fetches pending approvals over the direct link, and posts a
   **local** `UNNotificationRequest` — with Approve/Deny actions, exactly as a push
   would. Local notifications need no APNs, no auth key, no server, and no
   entitlement beyond the normal notification permission. Honest limitation: iOS
   schedules these at its own discretion, so delivery may lag by minutes or hours
   and is never guaranteed. Treat it as a *nudge*, never as the mechanism runs
   depend on.
3. **Nothing else works.** Ruled out: silent push, PushKit/VoIP push and critical
   alerts (all APNs); `NEAppPushProvider` (needs a special Apple entitlement); and
   holding a socket open in the background (iOS suspends it, and the background
   modes that would prevent that are for audio/location/VoIP — misusing them means
   rejection and battery drain).

**The architectural consequence — approvals become policy, not interruption.**

Since the daemon *cannot* reliably reach you, it must not depend on doing so. A
run that hits a permission gate can no longer just block forever waiting for a
human who may not open the app until tomorrow. So the approval object
grows a policy layer:

- **Pre-authorisation rules** per project and per agent profile — allow/deny
  patterns evaluated automatically, so most gates never become questions at all.
  This is where the leverage is: the best approval is the one never asked.
- **Timeout with an explicit default action** — deny-and-**park**, not deny-and-die.
  A parked run is resumable from the gate once you decide, so an unanswered
  approval costs latency, not work.
- **Batching** — pending approvals accumulate into a queue you triage when you open
  the app, rather than arriving one at a time.

This is a better design than push-driven interruption regardless, and it means push
can be added later as a pure latency optimisation over an already-correct system —
never as a load-bearing part of it.

**Reversibility.** Nothing here forecloses push. If the app matures and the
security posture is worth it, adding APNs means: obtain a `.p8`, add token storage,
and have the daemon send the same notifications it already composes for the local
path — while keeping the doorbell discipline (contentless payload, content fetched
over the direct link). The notification *content* pipeline is identical either way,
so build it once, source it locally now.

### 13.4 Pairing and auth

`aios pair` prints a QR on the Mac containing `{baseUrls[], deviceToken, caPin}`.
The app scans it and stores the token in the **iOS Keychain** (with
`kSecAttrAccessibleWhenUnlockedThisDeviceOnly`). Tokens are per-device, scoped,
listed by `aios device list`, and revocable with `aios device revoke`. Biometric
gate (Face ID) on app open, and required again for any approval action.

Layered with Tailscale you get network-level *and* application-level auth, which
is the right posture for something that can `rm -rf` your repos.

### 13.5 App form factor — **native SwiftUI (decided)**

Decided: a native SwiftUI app, `apps/ios/`, no cross-platform layer.

The objection to native was schema drift from hand-writing Swift models against a
daemon written in another language. **That objection is void** — the types are
generated, see §13.6
— so native keeps the shared-schema property *and* gains the platform integration
that actually matters here (Live Activities, notification actions, App Intents,
interactive widgets). Those are not garnish for this app: an agent run is exactly
the long-lived, glanceable, occasionally-interactive process Live Activities were
built for.

Because this is a single-operator personal app there is **no back-compat burden**:
target the newest iOS, use Swift Concurrency, the Observation framework
(`@Observable`), SwiftData, and current ActivityKit throughout. No deployment-target
gymnastics, no availability checks.

**App-side stack:**

| Concern | Choice |
| ------- | ------ |
| UI | SwiftUI, `@Observable` view models, `NavigationStack` |
| Networking | Generated OpenAPI client over `URLSession` (§13.8) |
| Streaming | SSE via `URLSession.bytes(for:)` → `for try await line in bytes.lines`, reconnect with `Last-Event-ID`. No third-party dependency. |
| Local store | SwiftData — run/message cache, and the offline **outbox** with idempotency keys |
| Secrets | Keychain with `kSecAttrAccessControl` + `.biometryCurrentSet`, so the device token is gated by Face ID at the Keychain layer, not just by app-level UI |
| Background | Push-driven; `BGTaskScheduler` only for opportunistic refresh |

### 13.6 Generated clients — how native keeps the shared schema

The §2 projection reaches Swift by **generation, not hand-writing**:

```
capability def  (Rust serde types + axum handlers)
   └─ utoipa ──────▶ openapi.json   (served at /openapi.json, committed)
                        ├─ swift-openapi-generator ──▶ AIOSKit client + models
                        └─ openapi-typescript ───────▶ clients/ts
```

`swift-openapi-generator` is Apple's own project and runs as a **SwiftPM build
plugin**, so Swift types regenerate on every Xcode build from the committed spec:
no generated code to review, no manual sync step, and a daemon-side schema change
that breaks a client breaks it **at compile time** rather than at runtime on your
phone. Recent versions emit `text/event-stream` bodies as an `AsyncSequence`, so
the run event stream is typed too.

This is *better* than the original TypeScript plan, where Zod schemas were a second
artifact to keep aligned with the implementation. In Rust the `serde` structs are
simultaneously the wire format, the OpenAPI schema, and the internal type — one
definition, no drift possible upstream of the spec.

Guardrails:

- `cargo xtask openapi` regenerates `openapi.json`.
- CI (or a pre-commit hook) fails if the committed spec is stale relative to the
  handlers. This is the single seam where drift could re-enter, so it gets a hard
  check.

### 13.7 Native capabilities worth building (no-push variants)

- **Live Activity per run** — Lock Screen and Dynamic Island showing the active run:
  current tool, elapsed, step count. Without APNs these update via `Activity.update()`
  **only while the app is running**, so treat them as a foreground/recently-active
  nicety rather than a background monitor. They still earn their place when you
  start a run from the phone and keep the screen nearby.
- **Local notification actions** — `UNNotificationCategory` with Approve/Deny
  `UNNotificationAction`s, posted locally (§13.3). Set `.authenticationRequired` on
  Approve so it demands an unlocked device; re-verify with LocalAuthentication for
  destructive scopes.
- **App Intents + entities** — expose `Project`, `Agent`, and `Run` as `AppEntity`
  so Siri and Shortcuts can name them: "ask ops about the billinga deploy".
  `AppShortcutsProvider` gives zero-config Siri phrases. Fully local, no server.
- **Interactive widgets** — WidgetKit + `AppIntent` buttons: pending-approval count,
  running agents, approve from the Home Screen. Widget timeline refreshes are
  opportunistic like background refresh, with the same caveat.
- **Share Sheet extension** — anything → vault `inbox/`.

### 13.8 Third parties in the data path — now zero

With push dropped and the relay removed, nothing external touches your data:

| Component | Third party involved? |
| --------- | --------------------- |
| App ↔ daemon on LAN | **None.** mDNS discovery, direct connection, packets never leave the house. |
| App ↔ daemon remote | **None in the data path.** Direct P2P WireGuard. Tailscale's coordinator brokers public keys and cannot decrypt; DERP is a ciphertext-only fallback. |
| Notifications | **None.** Composed and delivered locally on-device. |
| Harness model calls | Anthropic / OpenAI, unavoidably — that is what a coding agent *is*. Unchanged by any of this. |

Data-minimisation rules that follow, and should be enforced from Phase 0 rather
than retrofitted:

- No telemetry, no analytics, no crash reporting to anyone.
- No push tokens, no APNs key, no Apple Developer Program dependency for the
  daemon.
- All state stays in `~/.aios` and the two git repos you control.
- The app caches only what it needs to render offline, in an app-container
  SwiftData store, and wipes it on unpair.

### 13.9 What the app should actually contain

- **Inbox** — the approval triage queue, failed runs, agent messages. The default
  tab, and the app's reason to exist: batched decisions you clear in one pass.
- **Chat** — talk to any agent profile, streaming, project-scoped or global.
- **Runs** — live list, event stream per run, interrupt, view diff, approve/deny.
- **Projects** — registry browse, per-project issues and recent runs.
- **Capture** — Share Sheet extension → straight into the vault `inbox/`, and a
  quick-issue composer that hits beads.
- **App Intents / Shortcuts** — "ask <agent> …" and "capture to AIOS" exposed to
  Siri and the Shortcuts app, so automations on the phone can drive the OS.
- **Widget** — currently-running agents and pending approval count (refreshed
  opportunistically; not a reliable alert).

### 13.10 Impact on the phase plan

- **Phase 3** gains the biggest change: permission decisions become first-class
  addressable approval objects **with a policy layer** — pre-authorisation rules,
  timeout-to-parked, and resumability from the gate. Without push this is load-
  bearing, not optional: it is what stops a run stalling forever on a human who
  isn't looking.
- **Phase 4** gains: cursor-addressable resumable SSE; device pairing and token
  auth; the API-only rule from §3.1.
- **Phase 4.5**: committed OpenAPI spec + staleness check. Small, and it must land
  before any Swift is written.
- **Phase 6** carries the transport work (TLS, mDNS, Tailscale) *and* the iOS app —
  pairing, inbox, chat, runs, approval triage, local notifications, App Intents —
  on top of the `AIOSKit` layer the macOS app already established.
- Tailscale is an install-and-configure step, not a build task; mDNS advertisement
  is a small daemon feature.
- The Vercel relay is removed from the architecture.
- **No Apple Developer Program dependency for the daemon.** Note that device
  installation is a separate question from push: a free Apple account re-signs
  every 7 days, a paid one gives year-long provisioning and TestFlight. That is a
  convenience decision you can make at Phase 7, not an architectural one.

None of this changes phases 0–2.

---

## 14. Tier 1: the local desktop client (macOS first)

### 14.1 What it is

A native SwiftUI Mac app, `clients/apple/AIOS-macOS/`, that is the primary visual admin
surface — and the reason there is no Next.js UI in this plan. It is an **API client
like every other**, talking to the daemon over the Unix socket (§3.2), plus one
privileged extra responsibility: installing and supervising the daemon's
LaunchAgent.

It shares `AIOSKit` with the iOS app: generated OpenAPI client, models,
`@Observable` stores, SSE stream handling with cursor resume, keychain, outbox.
Roughly the whole networking and state layer is written once. What differs is
entirely presentation — macOS gets a dense multi-pane workspace, iOS gets a triage
queue.

### 14.2 What it does that a browser UI could not

- **Menu bar extra** — running agents, pending-approval count, and a global hotkey
  for quick-ask/quick-capture. Always present, never a tab you have to find. This
  alone is most of the argument for native.
- **Real filesystem integration** — Open in Xcode/Zed/Finder, drag a folder onto
  the window to register it as a project, QuickLook for artifacts.
- **Live run inspection** — streamed output, and a diff view of what an agent
  changed in its worktree before you approve a merge.
- **Approval triage inline** — the §7.1 queue, with the diff right beside it.
- **Daemon lifecycle** — install/start/stop, log tail, health, version, and
  in-place binary upgrade.
- **Vault browser** — read and edit KB notes without leaving the app, alongside the
  beads board for the same project.

### 14.3 Lifecycle and distribution

- The binary ships inside `AIOS.app/Contents/Resources/aios`, registered as a
  LaunchAgent through `SMAppService.agent(plistName:)` — the modern replacement for
  `SMJobBless`, requiring no privileged helper because a *user* agent is all we
  need.
- The app must handle **daemon absent** as a first-class state: offer to install and
  start, never assume, never crash. Likewise **version skew** between app and
  daemon — the OpenAPI spec carries a version and the app should say so plainly
  rather than failing obscurely.
- **The app must not be App Store sandboxed.** An admin tool that spawns coding
  agents across arbitrary repositories fundamentally cannot live inside the App
  Sandbox, and it is worth deciding that up front rather than fighting entitlements
  later. Distribution is Developer ID + hardened runtime + notarization, or — for a
  personal build straight from Xcode — neither, at the cost of a Gatekeeper prompt
  on first launch.
- The Mac app is **optional**. `aios daemon install` on a headless box gets the same
  result. Nothing in the core may depend on the GUI existing.

### 14.4 Why this ordering

The macOS app is cheaper than the iOS app by a wide margin: it needs no TLS, no
mDNS, no Tailscale, no device pairing and no push story, because it talks over a
Unix socket to a daemon on the same machine. It also builds the `AIOSKit` layer the
iOS app will then reuse. Building it first means the iOS app reduces to
transport, pairing, and a triage UI over an already-proven client library.
