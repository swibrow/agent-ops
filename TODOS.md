# TODOS

## P2 — Near-term

### Permission attention counter badge
Show a persistent badge in the tab bar: "⚡ 3 need attention" when agents are waiting for permission. Visible from any tab.
- **Why:** The core value prop is knowing when agents need you. Making that visible everywhere is a no-brainer.
- **Effort:** S
- **Depends on:** Activity tracking (already implemented)

### Debug panel (--debug flag)
When launched with `agent-ops --debug`, show a collapsible bottom panel with: last sync time, sync duration, sessions matched/missed, DB size, memory usage, and errors.
- **Why:** When something goes wrong, debug info should be in the TUI, not a log file.
- **Effort:** M
- **Depends on:** Tracing/logging (already implemented)

## P3 — Vision

### Cost estimation per session
Parse Claude's usage data or estimate token usage from message counts. Show per-session and per-project cost in the dashboard and detail views.
- **Why:** Running many agents adds up. Visibility into cost per session/project helps prioritize.
- **Effort:** M
- **Depends on:** Understanding Claude's usage file format (may not be stable)

### Event-driven architecture (replace polling)
Replace 3-second polling with filesystem watchers (`notify` crate on `~/.claude/sessions/`) and tmux hooks for instant state transitions.
- **Why:** Polling is inherently laggy (up to 3s stale) and wastes CPU.
- **Note:** Filesystem watcher is partially implemented (watches session file changes). Full event-driven would also need tmux hooks.
- **Effort:** L
- **Depends on:** Background sync (already implemented)

### Multi-machine support
SSH tunnel to remote tmux servers. Monitor agents running on other machines.
- **Effort:** XL
- **Depends on:** Abstracting the tmux data source

### Agent orchestration
Launch/kill Claude agents from the TUI. Requires careful permission design.
- **Effort:** XL

### Crates.io publication
Publish as `cargo install agent-ops`. Needs README, docs, stable API.
- **Effort:** M
