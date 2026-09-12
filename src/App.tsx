import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Terminal } from "./Terminal";
import { AgentIcon } from "./icons";
import { SettingsModal } from "./SettingsModal";
import { applyState, STATE_META, type Session, type SessionState } from "./sessions";
import {
  isConfigError,
  loadConfig,
  writeExampleConfig,
  type ConfigError,
  type ConfigView,
} from "./config";

const CONFIG_OVERRIDE = import.meta.env.VITE_AGENT_SESSION_CONFIG as string | undefined;

export default function App() {
  const [config, setConfig] = useState<ConfigView | null>(null);
  const [configError, setConfigError] = useState<ConfigError | null>(null);
  const [loading, setLoading] = useState(true);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [notices, setNotices] = useState<string[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const writerRef = useRef<((d: string) => void) | null>(null);

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      const view = await loadConfig(CONFIG_OVERRIDE);
      setConfig(view);
      setConfigError(null);
    } catch (error) {
      setConfig(null);
      setConfigError(
        isConfigError(error)
          ? error
          : { kind: "invalid", path: "(未知)", errors: [String(error)] },
      );
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  useEffect(() => {
    const unlisten = listen<{ id: string; state: SessionState }>("session-state", (e) => {
      setSessions((prev) => applyState(prev, e.payload.id, e.payload.state));
    });
    const unlistenNotice = listen<{ id: string; message: string }>("session-notice", (e) => {
      setNotices((prev) => [...prev, `${e.payload.id}: ${e.payload.message}`]);
    });
    return () => {
      unlisten.then((f) => f());
      unlistenNotice.then((f) => f());
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
    setActionError(null);
    try {
      const id = await invoke<string>("start_session", { host, agent, project: "" });
      setSessions((prev) => applyState(prev, id, "connecting"));
      setActive(id);
    } catch (error) {
      setActionError(String(error));
    }
  };

  const generateConfig = async () => {
    setActionError(null);
    try {
      await writeExampleConfig();
      await reload();
    } catch (error) {
      setActionError(String(error));
    }
  };

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-dot" />
          Agent Sessions
        </div>

        <section className="panel">
          <h2 className="panel-title">会话</h2>
          {sessions.length === 0 ? (
            <p className="muted">还没有会话</p>
          ) : (
            <ul className="session-list">
              {sessions.map((s) => (
                <li key={s.id}>
                  <button
                    className={`session${s.id === active ? " is-active" : ""}`}
                    onClick={() => setActive(s.id)}
                  >
                    <AgentIcon agent={s.id} />
                    <span className="session-id">{s.id}</span>
                    <span className={`badge tone-${STATE_META[s.state].tone}`}>
                      {STATE_META[s.state].label}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>

        <section className="panel panel-grow">
          <h2 className="panel-title">新建会话</h2>
          {loading ? (
            <p className="muted">正在加载配置…</p>
          ) : configError ? (
            <ConfigErrorPanel error={configError} onGenerate={generateConfig} onReload={reload} />
          ) : config ? (
            <>
              <div className="new-grid">
                {config.agents.map((a) => (
                  <div className="agent-group" key={a.id}>
                    <div className="agent-head">
                      <AgentIcon agent={a.id} size={20} />
                      <span className="agent-label">{a.label}</span>
                    </div>
                    <div className="agent-hosts">
                      {config.hosts.map((h) => (
                        <button
                          key={`${h.name}-${a.id}`}
                          className="new-btn"
                          title={h.host}
                          onClick={() => void start(h.name, a.id)}
                        >
                          {h.name}
                        </button>
                      ))}
                    </div>
                  </div>
                ))}
              </div>
              <button className="settings-btn" onClick={() => setSettingsOpen(true)}>
                ⚙ 编辑配置
              </button>
            </>
          ) : null}
        </section>

        {config && (
          <div className="source" title={config.source_path}>
            配置：{config.source_path}
          </div>
        )}
      </aside>

      <main className="main">
        {active ? (
          <>
            <div className="term-head">
              <AgentIcon agent={active} size={18} />
              <span className="term-title">{active}</span>
              <span className={`badge tone-${STATE_META[sessions.find((s) => s.id === active)?.state ?? "connecting"].tone}`}>
                {STATE_META[sessions.find((s) => s.id === active)?.state ?? "connecting"].label}
              </span>
            </div>
            <div className="notices">
              {notices.length === 0 ? (
                <span className="muted">—</span>
              ) : (
                notices.map((n, i) => <div key={i}>{n}</div>)
              )}
            </div>
            <Terminal onData={onData} onResize={onResize} registerWriter={registerWriter} />
          </>
        ) : (
          <div className="welcome">
            <div className="welcome-icons">
              {(config?.agents ?? []).slice(0, 5).map((a) => (
                <AgentIcon key={a.id} agent={a.id} size={34} />
              ))}
            </div>
            <h1>Agent Sessions</h1>
            <p>选择左侧的 host / agent 开始一个持久化会话。</p>
            <p className="muted">断网后重连会自动回到同一会话，并恢复此前的输出。</p>
            {actionError && <p className="error">{actionError}</p>}
          </div>
        )}
      </main>

      {settingsOpen && config && (
        <SettingsModal
          config={config}
          onClose={() => setSettingsOpen(false)}
          onSaved={() => void reload()}
        />
      )}
    </div>
  );
}

function ConfigErrorPanel({
  error,
  onGenerate,
  onReload,
}: {
  error: ConfigError;
  onGenerate: () => void;
  onReload: () => void;
}) {
  if (error.kind === "not_found") {
    return (
      <div className="config-error">
        <p className="error-title">未找到配置文件</p>
        <p className="muted">已查找以下位置：</p>
        <ul className="path-list">
          {error.searched.map((p) => (
            <li key={p}>
              <code>{p}</code>
            </li>
          ))}
        </ul>
        <div className="row">
          <button className="primary" onClick={onGenerate}>
            生成示例配置
          </button>
          <button onClick={onReload}>重新加载</button>
        </div>
      </div>
    );
  }

  return (
    <div className="config-error">
      <p className="error-title">配置文件无效</p>
      <p>
        <code>{error.path}</code>
      </p>
      <ul className="error-list">
        {error.errors.map((e) => (
          <li key={e}>{e}</li>
        ))}
      </ul>
      <div className="row">
        <button className="primary" onClick={onReload}>
          重新加载
        </button>
      </div>
    </div>
  );
}
