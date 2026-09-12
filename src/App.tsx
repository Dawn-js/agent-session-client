import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Terminal, type ThemeName } from "./Terminal";
import { AgentIcon } from "./icons";
import { FilePanel } from "./FilePanel";
import { SettingsModal } from "./SettingsModal";
import { applyState, removeSession, STATE_META, type Session, type SessionState } from "./sessions";
import {
  isConfigError,
  loadConfig,
  writeExampleConfig,
  type ConfigError,
  type ConfigView,
} from "./config";

const CONFIG_OVERRIDE = import.meta.env.VITE_AGENT_SESSION_CONFIG as string | undefined;

const THEME_KEY = "agent-sessions.theme";

function initialTheme(): ThemeName {
  return localStorage.getItem(THEME_KEY) === "light" ? "light" : "dark";
}

export default function App() {
  const [config, setConfig] = useState<ConfigView | null>(null);
  const [configError, setConfigError] = useState<ConfigError | null>(null);
  const [loading, setLoading] = useState(true);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [notices, setNotices] = useState<string[]>([]);
  const [active, setActive] = useState<string | null>(null);
  // 会话 id -> host。文件面板要知道当前会话连的是哪台机器，
  // 但 id 是按 agent 命名的（见 start_session），host 只能在这里记下来。
  const [hostOf, setHostOf] = useState<Record<string, string>>({});
  // 会话 id -> agent id。技能页要去 `~/.<agent>/skills` 找，同样从 id 里读不出来。
  const [agentOf, setAgentOf] = useState<Record<string, string>>({});
  const [actionError, setActionError] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [filesOpen, setFilesOpen] = useState(true);
  const [theme, setTheme] = useState<ThemeName>(initialTheme);
  const writerRef = useRef<((d: string) => void) | null>(null);
  // 用户已经关掉的会话 id。runner 可能还在飞行中（重连、退出），
  // 它发来的 state/output/notice 一律丢弃 —— 否则 applyState 会把行加回来。
  const closedRef = useRef<Set<string>>(new Set());

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

  // 主题只落在 <html data-theme> 上，剩下的全是 CSS 变量的事
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem(THEME_KEY, theme);
  }, [theme]);

  useEffect(() => {
    const unlisten = listen<{ id: string; state: SessionState }>("session-state", (e) => {
      const { id, state } = e.payload;
      if (closedRef.current.has(id)) return;
      // closed 是终态：行直接消失，而不是显示成"已关闭"
      if (state === "closed") {
        setSessions((prev) => removeSession(prev, id));
        return;
      }
      setSessions((prev) => applyState(prev, id, state));
    });
    const unlistenNotice = listen<{ id: string; message: string }>("session-notice", (e) => {
      if (closedRef.current.has(e.payload.id)) return;
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
      if (e.payload.id !== active || closedRef.current.has(e.payload.id)) return;
      writerRef.current?.(e.payload.data);
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
      setHostOf((prev) => ({ ...prev, [id]: host }));
      setAgentOf((prev) => ({ ...prev, [id]: agent }));
      // 重新开始同一个 id：撤掉墓碑，否则它的事件会被当成已关闭而丢弃
      closedRef.current.delete(id);
      setSessions((prev) => applyState(prev, id, "connecting"));
      setActive(id);
    } catch (error) {
      setActionError(String(error));
    }
  };

  const closeSession = async (id: string) => {
    // 先立墓碑再发关闭：runner 之后可能还发 exited / output 事件，
    // 不挡住的话 applyState 会把刚关掉的行又加回来。
    closedRef.current.add(id);
    try {
      // 只断开本地 ssh，远端 tmux 会话保留，之后还能接回来
      await invoke("close_session", { id, killRemote: false });
    } catch {
      // runner 已经 give-up 时后端已清表，会报 unknown session —— 那本来就是关掉的会话
    }
    setSessions((prev) => removeSession(prev, id));
    if (active === id) setActive(null);
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

        {/* 开关放在侧边栏：无论有没有活动会话都可见，
            否则隐藏后没有会话就再也开不回来了 */}
        <div className="side-toggles">
          <button
            className={`panel-toggle${filesOpen ? " is-on" : ""}`}
            onClick={() => setFilesOpen((open) => !open)}
            title={filesOpen ? "隐藏服务器文件栏" : "显示服务器文件栏"}
          >
            {filesOpen ? "▸ 隐藏文件栏" : "◂ 显示文件栏"}
          </button>
          <button
            className="panel-toggle"
            onClick={() => setTheme((t) => (t === "dark" ? "light" : "dark"))}
            title={theme === "dark" ? "切换到浅色主题" : "切换到深色主题"}
          >
            {theme === "dark" ? "浅色" : "深色"}
          </button>
        </div>

        <section className="panel">
          <h2 className="panel-title">会话</h2>
          {sessions.length === 0 ? (
            <p className="muted">还没有会话</p>
          ) : (
            <ul className="session-list">
              {sessions.map((s) => (
                <li key={s.id} className={`session${s.id === active ? " is-active" : ""}`}>
                  <button className="session-main" onClick={() => setActive(s.id)}>
                    <AgentIcon agent={s.id} />
                    <span className="session-id">{s.id}</span>
                    <span className={`badge tone-${STATE_META[s.state].tone}`}>
                      {STATE_META[s.state].label}
                    </span>
                  </button>
                  {/* 关闭按钮必须是兄弟节点：button 里不能再套 button */}
                  <button
                    className="session-close"
                    aria-label="关闭会话"
                    title="关闭会话（仅断开本地连接，远端 tmux 会话保留）"
                    onClick={() => void closeSession(s.id)}
                  >
                    ✕
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
            <Terminal
              onData={onData}
              onResize={onResize}
              registerWriter={registerWriter}
              theme={theme}
            />
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

      {filesOpen && (
        <FilePanel
          host={active ? hostOf[active] ?? null : null}
          agent={active ? agentOf[active] ?? null : null}
        />
      )}

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
