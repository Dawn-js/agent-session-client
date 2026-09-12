export type SessionState = "connecting" | "connected" | "retrying" | "exited" | "closed";

export interface Session {
  id: string;
  state: SessionState;
}

export function applyState(sessions: Session[], id: string, state: SessionState): Session[] {
  const idx = sessions.findIndex((s) => s.id === id);
  if (idx === -1) {
    return [...sessions, { id, state }];
  }
  return sessions.map((s, i) => (i === idx ? { ...s, state } : s));
}

/** Drop a session row (close button, or a `closed` event for a live session). */
export function removeSession(sessions: Session[], id: string): Session[] {
  return sessions.filter((s) => s.id !== id);
}

/** Human label + CSS tone for each connection state (used by the UI). */
export const STATE_META: Record<SessionState, { label: string; tone: string }> = {
  connecting: { label: "连接中", tone: "connecting" },
  connected: { label: "已连接", tone: "connected" },
  retrying: { label: "重连中", tone: "retrying" },
  exited: { label: "已退出", tone: "exited" },
  closed: { label: "已关闭", tone: "closed" },
};
