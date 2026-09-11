import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Terminal } from "./Terminal";
import { applyState, type Session, type SessionState } from "./sessions";

interface ConfigView {
  hosts: { name: string; host: string }[];
  agents: { id: string; label: string }[];
}

export default function App() {
  const [config, setConfig] = useState<ConfigView | null>(null);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const writerRef = useRef<((d: string) => void) | null>(null);

  useEffect(() => {
    invoke<ConfigView>("load_config", { path: "examples/config.example.json" })
      .then(setConfig)
      .catch((e) => console.error(e));

    const unlisten = listen<{ id: string; state: SessionState }>("session-state", (e) => {
      setSessions((prev) => applyState(prev, e.payload.id, e.payload.state));
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  useEffect(() => {
    if (!active) return;
    const un = listen<{ id: string; data: string }>("session-output", (e) => {
      if (e.payload.id === active) writerRef.current?.(e.payload.data);
    });
    return () => {
      un.then((f) => f());
    };
  }, [active]);

  const onData = useCallback(
    (data: string) => {
      if (active) void invoke("write_session", { id: active, data });
    },
    [active],
  );

  const onResize = useCallback(
    (cols: number, rows: number) => {
      if (active) void invoke("resize_session", { id: active, cols, rows });
    },
    [active],
  );

  const registerWriter = useCallback((write: (data: string) => void) => {
    writerRef.current = write;
  }, []);

  const start = async (host: string, agent: string) => {
    const id = await invoke<string>("start_session", { host, agent, project: "" });
    setSessions((prev) => applyState(prev, id, "connecting"));
    setActive(id);
  };

  return (
    <div style={{ display: "flex", height: "100vh", fontFamily: "system-ui" }}>
      <aside style={{ width: 220, borderRight: "1px solid #ddd", padding: 8 }}>
        <h3>Sessions</h3>
        {sessions.map((s) => (
          <button
            key={s.id}
            onClick={() => setActive(s.id)}
            style={{ display: "block", width: "100%", textAlign: "left" }}
          >
            {s.id} — {s.state}
          </button>
        ))}
        <h4>New</h4>
        {config?.hosts.map((h) =>
          config.agents.map((a) => (
            <button key={`${h.name}-${a.id}`} onClick={() => start(h.name, a.id)}>
              {h.name} / {a.label}
            </button>
          )),
        )}
      </aside>
      <main style={{ flex: 1, display: "flex", flexDirection: "column" }}>
        {active ? (
          <Terminal onData={onData} onResize={onResize} registerWriter={registerWriter} />
        ) : (
          <p style={{ padding: 16 }}>选一个 host / agent 开始会话</p>
        )}
      </main>
    </div>
  );
}
