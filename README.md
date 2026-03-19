```
  ╔═╗ ╔═╗ ╔═╗ ╔╗╔ ╔╦╗   ╔═╗ ╔═╗ ╔═╗
  ╠═╣ ║ ╦ ║╣  ║║║  ║    ║ ║ ╠═╝ ╚═╗
  ╩ ╩ ╚═╝ ╚═╝ ╝╚╝  ╩    ╚═╝ ╩   ╚═╝
  ─── monitor ∙ track ∙ resume ───────
```

**`htop` for your Claude Code agents.** A terminal UI that monitors every Claude Code session running in tmux — live activity, CPU/RAM, project history — and lets you jump into any session with one keypress.

## Why

You're running 8 Claude agents across 5 projects in tmux. One needs permission. Two finished 3 hours ago and you forgot. One is burning CPU on a loop. You have no idea which window is which.

agent-ops fixes that. Open it and instantly see everything.

## Install

### Homebrew

```sh
brew install swibrow/tap/agent-ops
```

### From source

```sh
cargo install --git https://github.com/swibrow/agent-ops
```

### From releases

Download the binary for your platform from [GitHub Releases](https://github.com/swibrow/agent-ops/releases).

## Usage

```sh
agent-ops
```

That's it. It discovers Claude sessions automatically via tmux.

## Features

### Dashboard

Live view of all running Claude agents with real-time status:

- **Activity detection** — see which agents are processing (braille spinner), idle (waiting for input), or need permission (flashing alert)
- **CPU & RAM** per agent — process tree stats updated every sync
- **30-day sparkline** — activity heatmap at a glance
- **Sessions sorted by urgency** — agents needing attention float to the top

### Projects

Every project you've ever used Claude with, ranked by staleness:

- **Staleness indicators** — Hot / Warm / Cool / Cold / Frozen / Forgotten
- **Sort** by name, last activity, session count, or staleness
- **Filter** to show only forgotten projects
- **Per-project sparklines** — 30-day activity trends inline
- **Open in editor** — press `e` to launch `$EDITOR`

### History

Timeline of every Claude prompt across all projects:

- **Date-grouped entries** with prompt previews
- **Filter by project** — press `f` on any entry
- **Activity heatmap** — 30-day global activity

### Session Details

Press `Enter` on any session to see:

- Project, branch, session ID, status, tmux location
- Duration with age colorization (green < 1h, yellow < 8h, red > 8h)
- CPU/RAM, message count, first prompt, summary
- **Live pane preview** — see what the agent is doing right now
- **Resume** (`r`) — jump to the tmux pane or relaunch with `--resume`
- **Copy session ID** (`y`) — straight to clipboard

### Notifications

Native macOS notifications when an agent transitions to "waiting for permission." Never miss a stuck agent again.

> Tip: Set notification style to **Alerts** in System Settings for persistent notifications that stay until dismissed.

### Search

Press `/` to fuzzy search across all project names and paths. Works on any tab.

## Keybindings

| Key | Action |
|-----|--------|
| `1` `2` `3` | Switch tabs |
| `Tab` / `h` `l` | Next / previous tab |
| `j` `k` / `↑` `↓` | Navigate |
| `g` / `G` | First / last item |
| `Enter` | Session details |
| `r` | Resume session |
| `y` | Copy session ID (in detail view) |
| `e` | Open in editor (projects tab) |
| `/` | Search |
| `s` | Cycle sort (projects tab) |
| `f` / `F` | Filter / clear filter |
| `Space` | Toggle pane preview |
| `?` | Help |
| `q` | Quit (with confirmation) |
| `Ctrl-C` | Quit immediately |

## How it works

```
tmux list-panes -a ──→ detect Claude agents (title prefix)
                         │
~/.claude/sessions/  ──→ match by PID / process tree / cwd
                         │
ps -eo pid,ppid,%cpu,rss → CPU/RAM for each agent tree
                         │
                    SQLite (upsert + aggregate)
                         │
                    Ratatui TUI ──→ your terminal
```

- **Single tmux call** — one `list-panes -a` instead of N+1 subprocess calls
- **Background sync** — data gathering runs in a tokio task, UI never blocks
- **Filesystem watcher** — instant updates when `~/.claude/sessions/` changes
- **Process tree stats** — sums CPU/RAM across all child processes per agent

## Data

All data is derived from files Claude Code already creates:

| Source | What |
|--------|------|
| `~/.claude/sessions/*.json` | Active session PIDs and working directories |
| `~/.claude/projects/*/sessions-index.json` | Historical session metadata |
| `~/.claude/history.jsonl` | Prompt history |
| tmux | Live pane state and process info |

Persistence: `~/.local/share/agent-ops/agent-ops.db` (SQLite). Delete it anytime — it rebuilds from Claude's files on next launch.

Logs: `~/.local/share/agent-ops/agent-ops.log`

## Requirements

- macOS (Linux support planned)
- tmux
- Claude Code running in tmux sessions

## License

MIT
