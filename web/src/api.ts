const BASE = '/api/v1';

export interface Session {
  session_id: string;
  agent_type: string;
  agent_label: string;
  agent_icon: string;
  project_path: string;
  project_name: string;
  started_at: number;
  ended_at: number | null;
  status: string;
  first_prompt: string | null;
  summary: string | null;
  git_branch: string | null;
  message_count: number;
  last_activity: number | null;
}

export interface Project {
  path: string;
  name: string;
  first_seen: number;
  last_activity: number;
  total_sessions: number;
  total_messages: number;
  is_active: boolean;
  staleness: string;
  staleness_indicator: string;
  daily_activity: number[];
}

export interface HistoryEntry {
  timestamp: number;
  project: string;
  display: string;
  session_id: string | null;
}

export interface Stats {
  active_sessions: number;
  total_sessions: number;
  active_projects: number;
  total_projects: number;
  agent_type_counts: { agent_type: string; label: string; count: number }[];
}

export interface ReviewSession {
  session_id: string;
  agent_type: string;
  started_at: number;
  last_activity: number;
  message_count: number;
  first_prompt: string | null;
  summary: string | null;
  git_branch: string | null;
  key_prompts: string[];
  input_tokens: number;
  output_tokens: number;
  cache_creation_tokens: number;
  cache_read_tokens: number;
}

export interface ReviewProject {
  project_path: string;
  project_name: string;
  session_count: number;
  message_count: number;
  first_activity: number;
  last_activity: number;
  agents: string[];
  branches: string[];
  sessions: ReviewSession[];
}

export interface Review {
  range: { from: number; to: number; label: string };
  totals: {
    projects: number;
    sessions: number;
    messages: number;
    user_messages: number;
    assistant_messages: number;
    agents: Record<string, number>;
    input_tokens: number;
    output_tokens: number;
    cache_creation_tokens: number;
    cache_read_tokens: number;
  };
  projects: ReviewProject[];
}

async function fetchJson<T>(url: string): Promise<T> {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return res.json();
}

export interface Conversation {
  session_id: string;
  project_path: string;
  project_name: string;
  first_prompt: string | null;
  summary: string | null;
  message_count: number | null;
  git_branch: string | null;
  created: string | null;
  modified: string | null;
}

export interface ContentBlock {
  type: string;
  text?: string;
  tool_name?: string;
  tool_input?: string;
  tool_result?: string;
}

export interface ChatMessage {
  role: string;
  content: ContentBlock[];
  timestamp: string | null;
  model: string | null;
}

export const api = {
  sessions: () => fetchJson<Session[]>(`${BASE}/sessions`),
  activeSessions: () => fetchJson<Session[]>(`${BASE}/sessions/active`),
  projects: () => fetchJson<Project[]>(`${BASE}/projects`),
  history: (project?: string, limit = 500) => {
    const params = new URLSearchParams();
    if (project) params.set('project', project);
    params.set('limit', String(limit));
    return fetchJson<HistoryEntry[]>(`${BASE}/history?${params}`);
  },
  stats: () => fetchJson<Stats>(`${BASE}/stats`),
  conversations: () => fetchJson<Conversation[]>(`${BASE}/conversations`),
  chatMessages: (sessionId: string) => fetchJson<ChatMessage[]>(`${BASE}/conversations/${sessionId}`),
  review: (fromMs?: number, toMs?: number, project?: string) => {
    const params = new URLSearchParams();
    if (fromMs !== undefined) params.set('from', String(fromMs));
    if (toMs !== undefined) params.set('to', String(toMs));
    if (project) params.set('project', project);
    const qs = params.toString();
    return fetchJson<Review>(`${BASE}/review${qs ? `?${qs}` : ''}`);
  },
};
