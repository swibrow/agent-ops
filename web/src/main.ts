import './style.css';
import { api, type Session, type Project, type HistoryEntry, type Stats, type ChatMessage, type Review, type ReviewProject } from './api';

type View = 'dashboard' | 'projects' | 'history' | 'review';

let currentView: View = 'dashboard';
let showAllSessions = false;

// Review tab state
type ReviewRange = '1h' | '24h' | '7d' | '30d';
let reviewRange: ReviewRange = '24h';
let reviewData: Review | null = null;
let reviewLoading = false;
let reviewError: string | null = null;
let reviewExpanded: Set<string> = new Set(); // project_path -> expanded

function rangeToMs(range: ReviewRange): number {
  switch (range) {
    case '1h': return 3_600_000;
    case '24h': return 86_400_000;
    case '7d': return 604_800_000;
    case '30d': return 2_592_000_000;
  }
}

// Chat view state
let selectedSessionId: string | null = null;
let chatMessages: ChatMessage[] | null = null;
let chatLoading = false;

// ── RPG open-world state ────────────────────────────────────────

const TILE = 32;
const WORLD_W = 24;
const WORLD_H = 16;

let rpgMode = false;
let playerX = 1;
let playerY = 1;
let playerDir: 'down' | 'up' | 'left' | 'right' = 'down';

interface Npc {
  session: Session;
  x: number;
  y: number;
  color: string;
}
let npcs: Npc[] = [];

// Dialog state
let talkingTo: Npc | null = null;
let dialogMessages: ChatMessage[] | null = null;
let dialogIndex = 0;
let dialogCharIndex = 0;
let dialogRevealed = false;
let dialogTypingTimer: ReturnType<typeof setInterval> | null = null;

// Auto-play state
let rpgPlaying = false;
let rpgPaused = false;
let rpgNpcOrder: Npc[] = []; // sorted by session timestamp
let rpgCurrentNpcIdx = 0;
let rpgWalkTimer: ReturnType<typeof setInterval> | null = null;
let rpgWalkPath: { x: number; y: number }[] = [];
let rpgMsgSpeed = 3000; // ms per message when auto-playing

const NPC_COLORS: Record<string, string> = {
  claude: '#cc7832',
  codex: '#10a37f',
  opencode: '#3b82f6',
  gemini: '#4285f4',
  aider: '#a855f7',
  unknown: '#8b949e',
};

function placeNpcs(sessions: Session[]) {
  npcs = [];
  const spots: [number, number][] = [];
  for (let y = 2; y < WORLD_H - 1; y += 3) {
    for (let x = 5; x < WORLD_W - 1; x += 4) {
      spots.push([x + (y % 2), y]);
    }
  }
  sessions.forEach((s, i) => {
    if (i >= spots.length) return;
    npcs.push({
      session: s,
      x: spots[i][0],
      y: spots[i][1],
      color: NPC_COLORS[s.agent_type] ?? NPC_COLORS.unknown,
    });
  });
  // Sort order by timestamp
  rpgNpcOrder = [...npcs].sort((a, b) => a.session.started_at - b.session.started_at);
}

function npcAt(x: number, y: number): Npc | undefined {
  return npcs.find(n => n.x === x && n.y === y);
}

// Simple pathfinding: walk to an adjacent cell of the target (not on the NPC)
function buildPath(fromX: number, fromY: number, toNpc: Npc): { x: number; y: number }[] {
  // Target: one tile adjacent to NPC
  const candidates = [
    { x: toNpc.x - 1, y: toNpc.y },
    { x: toNpc.x + 1, y: toNpc.y },
    { x: toNpc.x, y: toNpc.y - 1 },
    { x: toNpc.x, y: toNpc.y + 1 },
  ].filter(p => p.x >= 0 && p.x < WORLD_W && p.y >= 0 && p.y < WORLD_H && !npcAt(p.x, p.y));
  if (!candidates.length) return [];
  // Pick the closest candidate
  candidates.sort((a, b) => (Math.abs(a.x - fromX) + Math.abs(a.y - fromY)) - (Math.abs(b.x - fromX) + Math.abs(b.y - fromY)));
  const target = candidates[0];
  // Simple L-shaped path: horizontal then vertical
  const path: { x: number; y: number }[] = [];
  let cx = fromX, cy = fromY;
  while (cx !== target.x) {
    cx += cx < target.x ? 1 : -1;
    path.push({ x: cx, y: cy });
  }
  while (cy !== target.y) {
    cy += cy < target.y ? 1 : -1;
    path.push({ x: cx, y: cy });
  }
  return path;
}

// ── Sort state ──────────────────────────────────────────────────

type SortDir = 'asc' | 'desc';

interface SortState<K extends string> {
  col: K;
  dir: SortDir;
}

let sessionSort: SortState<SessionCol> = { col: 'started_at', dir: 'desc' };
let projectSort: SortState<ProjectCol> = { col: 'last_activity', dir: 'desc' };

type SessionCol = 'agent_type' | 'project_name' | 'status' | 'git_branch' | 'message_count' | 'started_at' | 'last_activity' | 'first_prompt';
type ProjectCol = 'name' | 'total_sessions' | 'total_messages' | 'staleness' | 'last_activity';

function toggleSort<K extends string>(state: SortState<K>, col: K): SortState<K> {
  if (state.col === col) {
    return { col, dir: state.dir === 'asc' ? 'desc' : 'asc' };
  }
  return { col, dir: 'desc' };
}

function sortIndicator<K extends string>(state: SortState<K>, col: K): string {
  if (state.col !== col) return '';
  return state.dir === 'asc' ? ' ▲' : ' ▼';
}

const STALENESS_ORDER: Record<string, number> = {
  HOT: 0, WARM: 1, COOL: 2, COLD: 3, FROZEN: 4, FORGOTTEN: 5,
};

function compareSessions(a: Session, b: Session, col: SessionCol, dir: SortDir): number {
  let cmp = 0;
  switch (col) {
    case 'agent_type': cmp = a.agent_type.localeCompare(b.agent_type); break;
    case 'project_name': cmp = a.project_name.localeCompare(b.project_name); break;
    case 'status': cmp = a.status.localeCompare(b.status); break;
    case 'git_branch': cmp = (a.git_branch ?? '').localeCompare(b.git_branch ?? ''); break;
    case 'message_count': cmp = a.message_count - b.message_count; break;
    case 'started_at': cmp = a.started_at - b.started_at; break;
    case 'last_activity': cmp = (a.last_activity ?? 0) - (b.last_activity ?? 0); break;
    case 'first_prompt': cmp = (a.first_prompt ?? '').localeCompare(b.first_prompt ?? ''); break;
  }
  return dir === 'asc' ? cmp : -cmp;
}

function compareProjects(a: Project, b: Project, col: ProjectCol, dir: SortDir): number {
  let cmp = 0;
  switch (col) {
    case 'name': cmp = a.name.localeCompare(b.name); break;
    case 'total_sessions': cmp = a.total_sessions - b.total_sessions; break;
    case 'total_messages': cmp = a.total_messages - b.total_messages; break;
    case 'staleness': cmp = (STALENESS_ORDER[a.staleness] ?? 9) - (STALENESS_ORDER[b.staleness] ?? 9); break;
    case 'last_activity': cmp = a.last_activity - b.last_activity; break;
  }
  return dir === 'asc' ? cmp : -cmp;
}

const app = document.querySelector<HTMLDivElement>('#app')!;

// ── Time formatting ─────────────────────────────────────────────

function relativeTime(ms: number): string {
  const now = Date.now();
  const diff = now - ms;
  if (diff < 0) return 'just now';
  const secs = Math.floor(diff / 1000);
  if (secs < 60) return `${secs}s ago`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

function formatDate(ms: number): string {
  return new Date(ms).toLocaleString();
}

function formatDateIso(iso: string): string {
  return new Date(iso).toLocaleString();
}

// ── Sparkline ───────────────────────────────────────────────────

function sparkline(data: number[], height = 20): string {
  if (!data.length || data.every(v => v === 0)) return '';
  const max = Math.max(...data, 1);
  const bars = data
    .map(v => {
      const h = Math.max(1, (v / max) * height);
      return `<span class="bar" style="height:${h}px"></span>`;
    })
    .join('');
  return `<span class="sparkline">${bars}</span>`;
}

// ── Escape HTML ─────────────────────────────────────────────────

function esc(s: string): string {
  const el = document.createElement('span');
  el.textContent = s;
  return el.innerHTML;
}

// ── Sortable header helper ──────────────────────────────────────

function th<K extends string>(label: string, col: K, state: SortState<K>, table: string): string {
  return `<th class="sortable" data-col="${col}" data-table="${table}">${label}${sortIndicator(state, col)}</th>`;
}

// ── Render functions ────────────────────────────────────────────

function renderHeader(): string {
  const tabs: { view: View; label: string }[] = [
    { view: 'dashboard', label: 'Sessions' },
    { view: 'projects', label: 'Projects' },
    { view: 'review', label: 'Review' },
    { view: 'history', label: 'History' },
  ];
  const navButtons = tabs
    .map(t => `<button data-view="${t.view}" class="${currentView === t.view ? 'active' : ''}">${t.label}</button>`)
    .join('');

  let controls = '';
  if (selectedSessionId) {
    controls = `<button id="chat-back" class="toggle-btn">Back</button>`;
  } else if (currentView === 'dashboard') {
    controls = `
      <button id="toggle-all" class="toggle-btn ${showAllSessions ? 'active' : ''}">${showAllSessions ? 'All' : 'Active'}</button>
      <button id="rpg-toggle" class="toggle-btn ${rpgMode ? 'active' : ''}">RPG</button>
    `;
  }

  return `
    <header>
      <h1><span class="refresh-dot"></span>agent-ops</h1>
      <div class="header-controls">
        ${controls}
        <nav>${navButtons}</nav>
      </div>
    </header>
  `;
}

function renderStats(stats: Stats): string {
  const pills = stats.agent_type_counts
    .map(a => `<span class="agent-badge ${a.agent_type}">${a.label} ${a.count}</span>`)
    .join('');
  return `
    <div class="stats-bar">
      <div class="stat-card">
        <div class="label">Active Sessions</div>
        <div class="value green">${stats.active_sessions}</div>
      </div>
      <div class="stat-card">
        <div class="label">Total Sessions</div>
        <div class="value blue">${stats.total_sessions}</div>
      </div>
      <div class="stat-card">
        <div class="label">Active Projects</div>
        <div class="value green">${stats.active_projects}</div>
      </div>
      <div class="stat-card">
        <div class="label">Total Projects</div>
        <div class="value blue">${stats.total_projects}</div>
      </div>
    </div>
    ${pills ? `<div class="agent-pills" style="margin-bottom:12px">${pills}</div>` : ''}
  `;
}

function renderSessions(sessions: Session[]): string {
  if (!sessions.length) {
    return `<div class="empty"><div class="icon">--</div>No sessions found</div>`;
  }
  const sorted = [...sessions].sort((a, b) => compareSessions(a, b, sessionSort.col, sessionSort.dir));
  const rows = sorted
    .map(s => `
      <tr class="clickable-row" data-session-id="${s.session_id}">
        <td><span class="agent-badge ${s.agent_type}">${s.agent_icon} ${s.agent_label}</span></td>
        <td title="${esc(s.project_path)}">${esc(s.project_name)}</td>
        <td><span class="status ${s.status}">${s.status.toUpperCase()}</span></td>
        <td>${s.git_branch ? esc(s.git_branch) : '<span style="color:var(--text-muted)">-</span>'}</td>
        <td>${s.message_count}</td>
        <td title="${formatDate(s.started_at)}">${relativeTime(s.started_at)}</td>
        <td>${s.last_activity ? relativeTime(s.last_activity) : '-'}</td>
        <td title="${s.first_prompt ? esc(s.first_prompt) : ''}" style="max-width:200px">${s.first_prompt ? esc(s.first_prompt) : '<span style="color:var(--text-muted)">-</span>'}</td>
      </tr>
    `)
    .join('');

  const s = sessionSort;
  return `
    <table>
      <thead>
        <tr>
          ${th('Agent', 'agent_type', s, 'sessions')}
          ${th('Project', 'project_name', s, 'sessions')}
          ${th('Status', 'status', s, 'sessions')}
          ${th('Branch', 'git_branch', s, 'sessions')}
          ${th('Msgs', 'message_count', s, 'sessions')}
          ${th('Started', 'started_at', s, 'sessions')}
          ${th('Last Active', 'last_activity', s, 'sessions')}
          ${th('Prompt', 'first_prompt', s, 'sessions')}
        </tr>
      </thead>
      <tbody>${rows}</tbody>
    </table>
  `;
}

function renderProjects(projects: Project[]): string {
  if (!projects.length) {
    return `<div class="empty"><div class="icon">--</div>No projects found</div>`;
  }
  const sorted = [...projects].sort((a, b) => compareProjects(a, b, projectSort.col, projectSort.dir));
  const rows = sorted
    .map(p => `
      <tr>
        <td title="${esc(p.path)}">${esc(p.name)}</td>
        <td>${p.total_sessions}</td>
        <td>${p.total_messages}</td>
        <td><span class="staleness ${p.staleness}">${p.staleness_indicator} ${p.staleness}</span></td>
        <td>${relativeTime(p.last_activity)}</td>
        <td>${sparkline(p.daily_activity)}</td>
      </tr>
    `)
    .join('');

  const s = projectSort;
  return `
    <table>
      <thead>
        <tr>
          ${th('Project', 'name', s, 'projects')}
          ${th('Sessions', 'total_sessions', s, 'projects')}
          ${th('Messages', 'total_messages', s, 'projects')}
          ${th('Staleness', 'staleness', s, 'projects')}
          ${th('Last Active', 'last_activity', s, 'projects')}
          <th>Activity (30d)</th>
        </tr>
      </thead>
      <tbody>${rows}</tbody>
    </table>
  `;
}

function renderHistory(entries: HistoryEntry[]): string {
  if (!entries.length) {
    return `<div class="empty"><div class="icon">--</div>No history entries</div>`;
  }
  const items = entries
    .map(e => `
      <div class="history-entry">
        <span class="time">${formatDate(e.timestamp)}</span>
        <span class="project" title="${esc(e.project)}">${esc(e.project.split('/').pop() || e.project)}</span>
        <span class="display">${esc(e.display)}</span>
      </div>
    `)
    .join('');
  return `<div class="history-list">${items}</div>`;
}

// ── Review ──────────────────────────────────────────────────────

async function loadReview() {
  reviewLoading = true;
  reviewError = null;
  reviewData = null;
  render();
  try {
    const now = Date.now();
    const from = now - rangeToMs(reviewRange);
    reviewData = await api.review(from, now);
  } catch (err) {
    reviewError = String(err);
  } finally {
    reviewLoading = false;
    render();
  }
}

function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

function renderReviewProject(p: ReviewProject): string {
  const isOpen = reviewExpanded.has(p.project_path);
  const branches = p.branches.length
    ? `<span class="review-branches">${p.branches.map(b => `<span class="review-branch">${esc(b)}</span>`).join('')}</span>`
    : '';
  const agents = p.agents.length
    ? `<span class="review-agents">${p.agents.map(a => `<span class="agent-badge ${esc(a)}">${esc(a)}</span>`).join('')}</span>`
    : '';

  const sessionsHtml = isOpen
    ? `<div class="review-sessions">${p.sessions.map(s => {
        const prompts = s.key_prompts.length
          ? `<ul class="review-prompts">${s.key_prompts.map(pr => `<li>${esc(pr)}</li>`).join('')}</ul>`
          : '';
        const title = s.summary || s.first_prompt || '(no summary)';
        const totalTok = s.input_tokens + s.output_tokens;
        const tokenBadge = totalTok > 0
          ? `<span class="review-token-badge" title="Input: ${fmtTokens(s.input_tokens)} · Output: ${fmtTokens(s.output_tokens)}${s.cache_read_tokens > 0 ? ` · Cache read: ${fmtTokens(s.cache_read_tokens)}` : ''}">${fmtTokens(s.input_tokens)}↑ ${fmtTokens(s.output_tokens)}↓</span>`
          : '';
        return `
          <div class="review-session">
            <div class="review-session-header">
              <span class="review-session-title">${esc(title)}</span>
              <span class="review-session-meta">
                ${s.message_count} msgs
                ${tokenBadge}
                ${s.git_branch ? ` · ${esc(s.git_branch)}` : ''}
                · ${relativeTime(s.last_activity)}
              </span>
            </div>
            ${prompts}
          </div>
        `;
      }).join('')}</div>`
    : '';

  return `
    <div class="review-project ${isOpen ? 'open' : ''}" data-project-path="${esc(p.project_path)}">
      <div class="review-project-header">
        <span class="review-project-toggle">${isOpen ? '▼' : '▶'}</span>
        <span class="review-project-name">${esc(p.project_name)}</span>
        <span class="review-project-stats">
          ${p.session_count} session${p.session_count === 1 ? '' : 's'} ·
          ${p.message_count} messages ·
          ${relativeTime(p.last_activity)}
        </span>
        ${agents}
        ${branches}
      </div>
      ${sessionsHtml}
    </div>
  `;
}

function reviewToMarkdown(r: Review): string {
  const lines: string[] = [];
  lines.push(`# Review — ${r.range.label}`);
  lines.push('');
  lines.push(`**${r.totals.projects}** projects · **${r.totals.sessions}** sessions · **${r.totals.messages}** messages`);
  lines.push('');
  for (const p of r.projects) {
    lines.push(`## ${p.project_name}`);
    lines.push(`- ${p.session_count} session(s), ${p.message_count} messages`);
    if (p.branches.length) lines.push(`- Branches: ${p.branches.join(', ')}`);
    for (const s of p.sessions) {
      const title = s.summary || s.first_prompt || '(no summary)';
      lines.push(`- **${title}**`);
      for (const prompt of s.key_prompts) {
        lines.push(`  - ${prompt}`);
      }
    }
    lines.push('');
  }
  return lines.join('\n');
}

function renderReview(): string {
  const rangeBtn = (r: ReviewRange, label: string) =>
    `<button class="review-range-btn ${reviewRange === r ? 'active' : ''}" data-range="${r}">${label}</button>`;

  const rangeBar = `
    <div class="review-controls">
      <div class="review-range-picker">
        ${rangeBtn('1h', '1h')}
        ${rangeBtn('24h', '24h')}
        ${rangeBtn('7d', '7d')}
        ${rangeBtn('30d', '30d')}
      </div>
      <div class="review-actions">
        <button id="review-refresh" class="toggle-btn">Refresh</button>
        ${reviewData ? '<button id="review-copy" class="toggle-btn">Copy Markdown</button>' : ''}
      </div>
    </div>
  `;

  let body = '';
  if (reviewLoading) {
    body = `<div class="empty">Loading review...</div>`;
  } else if (reviewError) {
    body = `<div class="empty"><div class="icon">!</div>Failed to load review: ${esc(reviewError)}</div>`;
  } else if (!reviewData) {
    body = `<div class="empty">Click a range to load a review.</div>`;
  } else if (reviewData.projects.length === 0) {
    body = `<div class="empty"><div class="icon">--</div>No activity in ${reviewData.range.label}</div>`;
  } else {
    const r = reviewData;
    const agentPills = Object.entries(r.totals.agents)
      .map(([a, c]) => `<span class="agent-badge ${esc(a)}">${esc(a)} ${c}</span>`)
      .join('');
    const totalTok = r.totals.input_tokens + r.totals.output_tokens;
    const tokenTooltip = `Input: ${fmtTokens(r.totals.input_tokens)} · Output: ${fmtTokens(r.totals.output_tokens)}${r.totals.cache_read_tokens > 0 ? ` · Cache read: ${fmtTokens(r.totals.cache_read_tokens)}` : ''}`;
    const summary = `
      <div class="review-summary">
        <div class="review-summary-row">
          <div class="stat-card">
            <div class="label">Range</div>
            <div class="value" style="font-size:16px">${esc(r.range.label)}</div>
          </div>
          <div class="stat-card">
            <div class="label">Projects</div>
            <div class="value blue">${r.totals.projects}</div>
          </div>
          <div class="stat-card">
            <div class="label">Sessions</div>
            <div class="value green">${r.totals.sessions}</div>
          </div>
          <div class="stat-card">
            <div class="label">Messages</div>
            <div class="value blue">${r.totals.messages}</div>
          </div>
          ${totalTok > 0 ? `
          <div class="stat-card" title="${tokenTooltip}">
            <div class="label">Tokens (in+out)</div>
            <div class="value magenta">${fmtTokens(totalTok)}</div>
          </div>` : ''}
        </div>
        ${agentPills ? `<div class="agent-pills">${agentPills}</div>` : ''}
      </div>
    `;
    const projectList = r.projects.map(renderReviewProject).join('');
    body = summary + `<div class="review-project-list">${projectList}</div>`;
  }

  return rangeBar + body;
}

// ── Chat view ───────────────────────────────────────────────────

function renderChatMessages(messages: ChatMessage[]): string {
  if (!messages.length) {
    return `<div class="empty"><div class="icon">--</div>No messages in this conversation</div>`;
  }
  const items = messages
    .map(m => {
      const blocks = m.content.map(block => {
        switch (block.type) {
          case 'text':
            return `<div class="chat-text">${esc(block.text ?? '')}</div>`;
          case 'tool_use':
            return `<div class="chat-tool-use"><span class="tool-name">${esc(block.tool_name ?? 'tool')}</span>${block.tool_input ? `<pre class="tool-input">${esc(block.tool_input)}</pre>` : ''}</div>`;
          case 'tool_result':
            return '';
          default:
            return '';
        }
      }).join('');

      const time = m.timestamp ? `<span class="chat-time">${formatDateIso(m.timestamp)}</span>` : '';
      const model = m.model ? `<span class="chat-model">${m.model}</span>` : '';

      return `
        <div class="chat-message ${m.role}">
          <div class="chat-role-line">
            <span class="chat-role">${m.role === 'user' ? 'You' : 'Assistant'}</span>
            ${model}${time}
          </div>
          <div class="chat-content">${blocks}</div>
        </div>
      `;
    })
    .join('');
  return `<div class="chat-thread">${items}</div>`;
}

// ── RPG Open World ──────────────────────────────────────────────

function getMsgText(msg: ChatMessage): string {
  for (const block of msg.content) {
    if (block.type === 'text' && block.text) {
      return block.text.length > 160 ? block.text.slice(0, 157) + '...' : block.text;
    }
    if (block.type === 'tool_use') {
      return `*uses ${block.tool_name}*`;
    }
  }
  return '...';
}

function renderRpgWorld(): string {
  let tilesHtml = '';
  for (let y = 0; y < WORLD_H; y++) {
    for (let x = 0; x < WORLD_W; x++) {
      const hash = (x * 7 + y * 13) % 5;
      const variant = hash === 0 ? 'flower1' : hash === 1 ? 'flower2' : hash === 2 ? 'dark' : '';
      tilesHtml += `<div class="rpg-tile ${variant}" style="left:${x * TILE}px;top:${y * TILE}px"></div>`;
    }
  }

  // Trees at edges
  for (let x = 0; x < WORLD_W; x++) {
    if ((x * 3 + 1) % 4 === 0) {
      tilesHtml += `<div class="rpg-tree" style="left:${x * TILE}px;top:0px">🌲</div>`;
      tilesHtml += `<div class="rpg-tree" style="left:${x * TILE}px;top:${(WORLD_H - 1) * TILE}px">🌲</div>`;
    }
  }

  // NPCs
  const npcHtml = npcs.map((n, i) => {
    const isTalking = talkingTo === n;
    const isVisited = rpgPlaying && rpgNpcOrder.indexOf(n) < rpgCurrentNpcIdx;
    const isCurrent = rpgPlaying && rpgNpcOrder[rpgCurrentNpcIdx] === n;
    const name = n.session.project_name.length > 12 ? n.session.project_name.slice(0, 11) + '..' : n.session.project_name;
    const icon = n.session.agent_icon;
    const ts = formatDate(n.session.started_at);

    let bubble = '';
    if (isTalking && dialogMessages && dialogMessages[dialogIndex]) {
      const msg = dialogMessages[dialogIndex];
      const fullText = getMsgText(msg);
      const text = dialogRevealed ? fullText : fullText.slice(0, dialogCharIndex);
      const who = msg.role === 'user' ? 'You' : name;
      bubble = `<div class="rpg-bubble ${msg.role}"><span class="rpg-bubble-who">${esc(who)}:</span> ${esc(text)}${!dialogRevealed ? '<span class="rpg-cursor">|</span>' : ''}</div>`;
    }

    const visitedClass = isVisited ? 'visited' : '';
    const currentClass = isCurrent ? 'current' : '';

    return `
      <div class="rpg-npc ${isTalking ? 'talking' : ''} ${visitedClass} ${currentClass}" style="left:${n.x * TILE}px;top:${n.y * TILE}px" data-npc-idx="${i}">
        ${bubble}
        <div class="rpg-npc-label" style="color:${n.color}">${icon} ${esc(name)}</div>
        <div class="rpg-npc-sprite" style="background:${n.color}">
          <div class="rpg-npc-head"></div>
          <div class="rpg-npc-body" style="background:${n.color}"></div>
        </div>
        <div class="rpg-npc-time">${ts}</div>
      </div>
    `;
  }).join('');

  // Player (CSS transition handles the walking animation)
  const dirClass = `dir-${playerDir}`;
  const playerHtml = `
    <div class="rpg-player ${dirClass}" style="left:${playerX * TILE}px;top:${playerY * TILE}px">
      <div class="rpg-player-sprite">
        <div class="rpg-player-head"></div>
        <div class="rpg-player-body"></div>
      </div>
    </div>
  `;

  // Status bar
  const npcIdx = rpgCurrentNpcIdx;
  const total = rpgNpcOrder.length;
  const currentNpc = rpgNpcOrder[npcIdx];
  const statusText = rpgPlaying
    ? (talkingTo
      ? `Talking to ${currentNpc?.session.project_name ?? '?'} (${dialogIndex + 1}/${dialogMessages?.length ?? '?'})`
      : `Walking to ${currentNpc?.session.project_name ?? '?'}...`)
    : total > 0
      ? `${total} sessions sorted by time. Press Play to watch the replay.`
      : 'No sessions to replay.';

  const pauseLabel = rpgPaused ? '&#9654; Resume' : '&#10074;&#10074; Pause';

  const statusBar = `
    <div class="rpg-dialog-box">
      <div class="rpg-dialog-footer">
        <span class="rpg-hint">${statusText}</span>
        <span class="rpg-controls">
          <span class="rpg-counter">${rpgPlaying ? `${npcIdx + 1}/${total}` : ''}</span>
          ${!rpgPlaying ? `<button id="rpg-play" class="rpg-btn">&#9654; Play</button>` : `<button id="rpg-pause" class="rpg-btn">${pauseLabel}</button>`}
          ${rpgPlaying ? `<button id="rpg-skip" class="rpg-btn">&#9654;&#9654; Skip</button>` : ''}
          ${rpgPlaying ? `<button id="rpg-stop" class="rpg-btn">&#9632; Stop</button>` : ''}
        </span>
      </div>
    </div>
  `;

  return `
    <div class="rpg-world-container">
      <div class="rpg-world" style="width:${WORLD_W * TILE}px;height:${WORLD_H * TILE}px">
        ${tilesHtml}${npcHtml}${playerHtml}
      </div>
      ${statusBar}
    </div>
  `;
}

// ── Dialog typing ───────────────────────────────────────────────

function stopDialog() {
  if (dialogTypingTimer) { clearInterval(dialogTypingTimer); dialogTypingTimer = null; }
}

function startDialogTyping(onDone?: () => void) {
  stopDialog();
  if (!dialogMessages || !dialogMessages[dialogIndex]) { onDone?.(); return; }
  dialogCharIndex = 0;
  dialogRevealed = false;
  const fullText = getMsgText(dialogMessages[dialogIndex]);

  dialogTypingTimer = setInterval(() => {
    dialogCharIndex += 2;
    if (dialogCharIndex >= fullText.length) {
      dialogCharIndex = fullText.length;
      dialogRevealed = true;
      stopDialog();
      onDone?.();
    }
    updateBubbleInPlace();
  }, 30);
}

function updateBubbleInPlace() {
  if (!talkingTo || !dialogMessages) return;
  const bubbleEl = document.querySelector('.rpg-bubble');
  if (bubbleEl && dialogMessages[dialogIndex]) {
    const msg = dialogMessages[dialogIndex];
    const fullText = getMsgText(msg);
    const text = dialogRevealed ? fullText : fullText.slice(0, dialogCharIndex);
    const name = talkingTo.session.project_name.length > 12 ? talkingTo.session.project_name.slice(0, 11) + '..' : talkingTo.session.project_name;
    const who = msg.role === 'user' ? 'You' : name;
    bubbleEl.className = `rpg-bubble ${msg.role}`;
    bubbleEl.innerHTML = `<span class="rpg-bubble-who">${esc(who)}:</span> ${esc(text)}${!dialogRevealed ? '<span class="rpg-cursor">|</span>' : ''}`;
  }
  const counterEl = document.querySelector('.rpg-counter');
  if (counterEl) counterEl.textContent = `${rpgCurrentNpcIdx + 1}/${rpgNpcOrder.length}`;
  const hintEl = document.querySelector('.rpg-hint');
  if (hintEl && talkingTo) {
    hintEl.textContent = `Talking to ${talkingTo.session.project_name} (${dialogIndex + 1}/${dialogMessages?.length ?? '?'})`;
  }
}

// ── Auto-play engine ────────────────────────────────────────────

function rpgStopAll() {
  stopDialog();
  if (rpgWalkTimer) { clearInterval(rpgWalkTimer); rpgWalkTimer = null; }
  rpgPlaying = false;
  rpgPaused = false;
  talkingTo = null;
  dialogMessages = null;
  dialogIndex = 0;
  rpgWalkPath = [];
}

async function rpgStartReplay() {
  rpgStopAll();
  if (rpgNpcOrder.length === 0) return;
  rpgPlaying = true;
  rpgPaused = false;
  rpgCurrentNpcIdx = 0;
  playerX = 1;
  playerY = 1;
  renderPage();
  await rpgVisitNext();
}

async function rpgVisitNext() {
  if (!rpgPlaying || rpgCurrentNpcIdx >= rpgNpcOrder.length) {
    rpgStopAll();
    renderPage();
    return;
  }

  const npc = rpgNpcOrder[rpgCurrentNpcIdx];
  const path = buildPath(playerX, playerY, npc);
  rpgWalkPath = path;

  // Walk step by step
  await rpgWalkAlongPath();
  if (!rpgPlaying) return;

  // Face the NPC
  if (npc.x > playerX) playerDir = 'right';
  else if (npc.x < playerX) playerDir = 'left';
  else if (npc.y > playerY) playerDir = 'down';
  else playerDir = 'up';

  // Load and play conversation
  talkingTo = npc;
  dialogIndex = 0;
  renderPage();

  try {
    dialogMessages = await api.chatMessages(npc.session.session_id);
  } catch {
    dialogMessages = [];
  }

  if (!dialogMessages || dialogMessages.length === 0) {
    talkingTo = null;
    dialogMessages = null;
    rpgCurrentNpcIdx++;
    renderPage();
    await rpgVisitNext();
    return;
  }

  renderPage();
  // Auto-play messages
  await rpgAutoPlayMessages();
  if (!rpgPlaying) return;

  // Done talking, move to next
  talkingTo = null;
  dialogMessages = null;
  dialogIndex = 0;
  rpgCurrentNpcIdx++;
  renderPage();

  // Small pause between NPCs
  await sleep(500);
  if (!rpgPlaying) return;

  await rpgVisitNext();
}

function rpgWalkAlongPath(): Promise<void> {
  return new Promise(resolve => {
    if (rpgWalkPath.length === 0) { resolve(); return; }
    let stepIdx = 0;
    rpgWalkTimer = setInterval(() => {
      if (rpgPaused) return; // freeze while paused
      if (!rpgPlaying || stepIdx >= rpgWalkPath.length) {
        if (rpgWalkTimer) clearInterval(rpgWalkTimer);
        rpgWalkTimer = null;
        resolve();
        return;
      }
      const step = rpgWalkPath[stepIdx];
      // Update direction
      if (step.x > playerX) playerDir = 'right';
      else if (step.x < playerX) playerDir = 'left';
      else if (step.y > playerY) playerDir = 'down';
      else if (step.y < playerY) playerDir = 'up';
      playerX = step.x;
      playerY = step.y;
      stepIdx++;
      // Update player position via DOM for smooth animation
      const el = document.querySelector('.rpg-player') as HTMLElement;
      if (el) {
        el.style.left = `${playerX * TILE}px`;
        el.style.top = `${playerY * TILE}px`;
        el.className = `rpg-player dir-${playerDir}`;
      }
    }, 150);
  });
}

function rpgAutoPlayMessages(): Promise<void> {
  return new Promise(resolve => {
    const playNext = () => {
      if (!rpgPlaying || !dialogMessages) { resolve(); return; }
      renderPage();
      startDialogTyping(() => {
        // After typing finishes, wait then advance
        setTimeout(() => {
          if (!rpgPlaying) { resolve(); return; }
          if (rpgPaused) {
            // Wait for unpause
            const check = setInterval(() => {
              if (!rpgPaused || !rpgPlaying) { clearInterval(check); afterPause(); }
            }, 100);
            const afterPause = () => {
              if (!rpgPlaying) { resolve(); return; }
              advance();
            };
            return;
          }
          advance();
        }, rpgMsgSpeed);
      });
    };
    const advance = () => {
      if (!dialogMessages || dialogIndex >= dialogMessages.length - 1) {
        resolve();
      } else {
        dialogIndex++;
        playNext();
      }
    };
    playNext();
  });
}

function rpgSkipToNext() {
  // Skip current NPC conversation, move to next
  stopDialog();
  talkingTo = null;
  dialogMessages = null;
  dialogIndex = 0;
  if (rpgWalkTimer) { clearInterval(rpgWalkTimer); rpgWalkTimer = null; }
  // Jump player to next NPC's position
  rpgCurrentNpcIdx++;
  if (rpgCurrentNpcIdx >= rpgNpcOrder.length) {
    rpgStopAll();
    renderPage();
    return;
  }
  const npc = rpgNpcOrder[rpgCurrentNpcIdx];
  const adj = [
    { x: npc.x - 1, y: npc.y },
    { x: npc.x + 1, y: npc.y },
    { x: npc.x, y: npc.y - 1 },
    { x: npc.x, y: npc.y + 1 },
  ].filter(p => p.x >= 0 && p.x < WORLD_W && p.y >= 0 && p.y < WORLD_H && !npcAt(p.x, p.y));
  if (adj.length) { playerX = adj[0].x; playerY = adj[0].y; }
  renderPage();
  rpgVisitNext();
}

function sleep(ms: number): Promise<void> {
  return new Promise(r => setTimeout(r, ms));
}

// ── Main render ─────────────────────────────────────────────────

function renderPage() {
  // Build content without fetching (sync render for RPG)
  if (rpgMode && currentView === 'dashboard' && !selectedSessionId) {
    app.innerHTML = renderHeader() + renderRpgWorld();
    attachListeners();
    return;
  }
}

async function render() {
  try {
    let content = '';
    switch (currentView) {
      case 'dashboard': {
        if (selectedSessionId && chatMessages) {
          content = renderChatMessages(chatMessages);
        } else if (chatLoading) {
          content = `<div class="empty">Loading conversation...</div>`;
        } else if (rpgMode) {
          const sessions = showAllSessions ? await api.sessions() : await api.activeSessions();
          if (npcs.length === 0) {
            placeNpcs(sessions);
          }
          app.innerHTML = renderHeader() + renderRpgWorld();
          attachListeners();
          return;
        } else {
          const sessionFetch = showAllSessions ? api.sessions() : api.activeSessions();
          const [stats, sessions] = await Promise.all([api.stats(), sessionFetch]);
          content = renderStats(stats) + renderSessions(sessions);
        }
        break;
      }
      case 'projects': {
        const projects = await api.projects();
        content = renderProjects(projects);
        break;
      }
      case 'history': {
        const entries = await api.history();
        content = renderHistory(entries);
        break;
      }
      case 'review': {
        // Kick off a load if we don't have data yet for this range
        if (reviewData === null && !reviewLoading && !reviewError) {
          loadReview();
        }
        content = renderReview();
        break;
      }
    }
    app.innerHTML = renderHeader() + content;
  } catch (err) {
    app.innerHTML = renderHeader() + `<div class="empty"><div class="icon">!</div>Failed to fetch data: ${err}</div>`;
  }

  attachListeners();
}

function attachListeners() {
  const toggleBtn = app.querySelector<HTMLButtonElement>('#toggle-all');
  if (toggleBtn) {
    toggleBtn.addEventListener('click', () => {
      showAllSessions = !showAllSessions;
      render();
    });
  }

  const backBtn = app.querySelector<HTMLButtonElement>('#chat-back');
  if (backBtn) {
    backBtn.addEventListener('click', () => {
      stopDialog();
      selectedSessionId = null;
      chatMessages = null;
      render();
    });
  }

  const rpgToggle = app.querySelector<HTMLButtonElement>('#rpg-toggle');
  if (rpgToggle) {
    rpgToggle.addEventListener('click', () => {
      rpgMode = !rpgMode;
      if (!rpgMode) {
        rpgStopAll();
      }
      render();
    });
  }

  // RPG controls
  const playBtn = app.querySelector<HTMLButtonElement>('#rpg-play');
  if (playBtn) playBtn.addEventListener('click', () => rpgStartReplay());

  const pauseBtn = app.querySelector<HTMLButtonElement>('#rpg-pause');
  if (pauseBtn) {
    pauseBtn.addEventListener('click', () => {
      rpgPaused = !rpgPaused;
      renderPage();
    });
  }

  const skipBtn = app.querySelector<HTMLButtonElement>('#rpg-skip');
  if (skipBtn) skipBtn.addEventListener('click', () => rpgSkipToNext());

  const stopBtn = app.querySelector<HTMLButtonElement>('#rpg-stop');
  if (stopBtn) {
    stopBtn.addEventListener('click', () => {
      rpgStopAll();
      renderPage();
    });
  }

  // Session row click → open chat
  app.querySelectorAll<HTMLTableRowElement>('.clickable-row').forEach(row => {
    row.addEventListener('click', async () => {
      const sid = row.dataset.sessionId!;
      selectedSessionId = sid;
      chatLoading = true;
      chatMessages = null;
      render();
      try {
        chatMessages = await api.chatMessages(sid);
      } catch {
        chatMessages = [];
      }
      chatLoading = false;
      render();
    });
  });

  // Nav
  app.querySelectorAll<HTMLButtonElement>('nav button').forEach(btn => {
    btn.addEventListener('click', () => {
      const newView = btn.dataset.view as View;
      if (newView !== currentView) {
        currentView = newView;
        selectedSessionId = null;
        chatMessages = null;
        rpgStopAll();
        render();
      }
    });
  });

  // Sort
  app.querySelectorAll<HTMLTableCellElement>('th.sortable').forEach(header => {
    header.addEventListener('click', () => {
      const col = header.dataset.col!;
      const table = header.dataset.table!;
      if (table === 'sessions') {
        sessionSort = toggleSort(sessionSort, col as SessionCol);
      } else if (table === 'projects') {
        projectSort = toggleSort(projectSort, col as ProjectCol);
      }
      render();
    });
  });

  // Review: range picker
  app.querySelectorAll<HTMLButtonElement>('.review-range-btn').forEach(btn => {
    btn.addEventListener('click', () => {
      const r = btn.dataset.range as ReviewRange;
      if (r && r !== reviewRange) {
        reviewRange = r;
        reviewExpanded.clear();
        loadReview();
      }
    });
  });

  // Review: refresh
  const refreshBtn = app.querySelector<HTMLButtonElement>('#review-refresh');
  if (refreshBtn) {
    refreshBtn.addEventListener('click', () => loadReview());
  }

  // Review: copy as markdown
  const copyBtn = app.querySelector<HTMLButtonElement>('#review-copy');
  if (copyBtn) {
    copyBtn.addEventListener('click', () => {
      if (!reviewData) return;
      navigator.clipboard.writeText(reviewToMarkdown(reviewData)).then(() => {
        copyBtn.textContent = 'Copied!';
        setTimeout(() => { copyBtn.textContent = 'Copy Markdown'; }, 1500);
      });
    });
  }

  // Review: expand/collapse project
  app.querySelectorAll<HTMLDivElement>('.review-project-header').forEach(header => {
    header.addEventListener('click', () => {
      const project = header.closest('.review-project') as HTMLElement | null;
      const path = project?.dataset.projectPath;
      if (!path) return;
      if (reviewExpanded.has(path)) {
        reviewExpanded.delete(path);
      } else {
        reviewExpanded.add(path);
      }
      render();
    });
  });
}

// ── Bootstrap ───────────────────────────────────────────────────

render();
setInterval(() => {
  if (!selectedSessionId && !rpgMode && currentView !== 'review') {
    render();
  }
}, 3000);
