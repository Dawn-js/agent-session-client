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
