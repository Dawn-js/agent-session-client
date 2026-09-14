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
  listSshHosts,
  loadConfig,
  probeAgents,
  saveConfig,
  writeExampleConfig,
  type AgentView,
  type ConfigError,
  type ConfigView,
  type SshHostView,
} from "./config";
import { bootstrapConfigJson } from "./configEdit";

const CONFIG_OVERRIDE = import.meta.env.VITE_AGENT_SESSION_CONFIG as string | undefined;

const THEME_KEY = "agent-sessions.theme";

function initialTheme(): ThemeName {
  return localStorage.getItem(THEME_KEY) === "light" ? "light" : "dark";
}

export default function App() {
  const [config, setConfig] = useState<ConfigView | null>(null);
  const [configError, setConfigError] = useState<ConfigError | null>(null);
  // 本机 ~/.ssh/config 里已有的 Host 别名。首次启动没配置时用它代替"填地址"。
  const [sshHosts, setSshHosts] = useState<SshHostView[]>([]);
  const [loading, setLoading] = useState(true);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [notices, setNotices] = useState<string[]>([]);
  const [active, setActive] = useState<string | null>(null);
  // 会话 id -> host。文件面板要知道当前会话连的是哪台机器，
  // 但 id 是按 agent 命名的（见 start_session），host 只能在这里记下来。
  const [hostOf, setHostOf] = useState<Record<string, string>>({});
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
      setSshHosts([]);
    } catch (error) {
      setConfig(null);
      const failure: ConfigError = isConfigError(error)
        ? error
        : { kind: "invalid", path: "(未知)", errors: [String(error)] };
      setConfigError(failure);
      // 没配置时顺带列出本机 ssh config 里已有的服务器。读不到就当没有 ——
      // 这只是个便利入口，绝不能因此挡住启动。
      if (failure.kind === "not_found") {
        try {
          setSshHosts(await listSshHosts());
        } catch {
          setSshHosts([]);
        }
      } else {
        setSshHosts([]);
      }
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

  // 用 ref 读 active，让下面两个回调保持稳定：它们进 Terminal 的 effect 依赖，
  // 一旦随 active 变化就会重建 xterm —— 而 xterm 的 dispose 不摘自己插入的 DOM，
  // 重建出来的新实例会和残留的旧实例叠在一起（屏幕上的字看着像重影）。
  const activeRef = useRef<string | null>(null);
  useEffect(() => {
    activeRef.current = active;
  }, [active]);

  const onData = useCallback((data: string) => {
    const id = activeRef.current;
    if (id) void invoke("write_session", { id, data });
  }, []);

  const onResize = useCallback((cols: number, rows: number) => {
    const id = activeRef.current;
    if (id) void invoke("resize_session", { id, cols, rows });
  }, []);

  const registerWriter = useCallback((write: (data: string) => void) => {
    writerRef.current = write;
  }, []);

  // hostOf 也要能稳定读到（onScroll 不能随它变化，否则终端会被重建）
  const hostOfRef = useRef(hostOf);
  useEffect(() => {
    hostOfRef.current = hostOf;
  }, [hostOf]);

  // 滚轮直接命令 tmux 滚 copy-mode：用户环境里 Ctrl+b 这类按键到不了 tmux，
  // 但"执行一条远端命令"这条路是通的（文件面板一直在用）。
  const lastScrollAt = useRef(0);
  const onScroll = useCallback((up: boolean) => {
    const id = activeRef.current;
    const host = id ? hostOfRef.current[id] : undefined;
    if (!id || !host) return;
    // 滚轮会连着触发，每次都是一个 ssh 往返，节流一下
    const now = Date.now();
    if (now - lastScrollAt.current < 80) return;
    lastScrollAt.current = now;
    void invoke("scroll_session", { host, session: id, up, lines: 3 });
  }, []);

  const start = async (host: string, agent: string) => {
    setActionError(null);
    try {
      const id = await invoke<string>("start_session", { host, agent, project: "" });
      setHostOf((prev) => ({ ...prev, [id]: host }));
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
      // 结束会话 = 连远端 tmux 一起销毁（tmux kill-session），不留残留进程。
      // 下次再开是全新会话，不会被上一次的输出刷屏。
      await invoke("close_session", { id, killRemote: true });
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
      // 生成的模板里全是 REPLACE_WITH_* 占位符，必须逼用户立刻换成真值，
      // 否则又会变成一个"看着能用、点了必挂"的主机。
      setSettingsOpen(true);
    } catch (error) {
      setActionError(String(error));
    }
  };

  /**
   * 首次启动的探测与确认流程都在 `ConfigErrorPanel` 里 —— 它天然持有
   * 「当前这台选中的别名 + 探测状态」，放在那里不用把状态提上来。
   * 这里只提供保存后重新加载。
   */
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
                    aria-label="结束会话"
                    title="结束会话（销毁远端 tmux 会话及其中运行的 agent 进程）"
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
            <ConfigErrorPanel
              error={configError}
              sshHosts={sshHosts}
              onGenerate={generateConfig}
              onReload={reload}
            />
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
            {/* key 绑到会话 id：切换会话要换一个终端实例，否则上一个会话的画面
                会留在屏上，新会话的输出直接叠上去 */}
            <Terminal
              key={active}
              onData={onData}
              onResize={onResize}
              registerWriter={registerWriter}
              onScroll={onScroll}
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
        <FilePanel host={active ? hostOf[active] ?? null : null} />
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

/** 主机的可读描述，用作按钮 tooltip（别名本身才是连的东西）。 */
function describeSshHost(h: SshHostView): string {
  let target = h.host_name ?? h.alias;
  if (h.user) target = `${h.user}@${target}`;
  if (h.port) target = `${target}:${h.port}`;
  return h.proxy_jump ? `${target}（经 ${h.proxy_jump}）` : target;
}

function ConfigErrorPanel({
  error,
  sshHosts,
  onGenerate,
  onReload,
}: {
  error: ConfigError;
  sshHosts: SshHostView[];
  onGenerate: () => void;
  onReload: () => void;
}) {
  // 首次启动引导：选中别名后**先探测**那台机器装了哪些 agent，把结果摆出来让
  // 用户确认，只把探到的写进配置。以前这里直接写注册表全集，用户点进去看到的
  // 是一堆服务器上根本没装的 agent（点了必挂），而且全程没有任何探测和引导。
  const [probingAlias, setProbingAlias] = useState<string | null>(null);
  const [result, setResult] = useState<{ alias: string; agents: AgentView[] } | null>(null);
  const [probeError, setProbeError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const probe = async (alias: string) => {
    setProbingAlias(alias);
    setResult(null);
    setProbeError(null);
    try {
      setResult({ alias, agents: await probeAgents(alias) });
    } catch (e) {
      setProbeError(String(e));
    } finally {
      setProbingAlias(null);
    }
  };

  const confirm = async () => {
    if (!result) return;
    const json = bootstrapConfigJson(result.alias, result.agents);
    if (!json) return; // 一个都没探到：按钮本来就不给点，这里兜底
    setSaving(true);
    setProbeError(null);
    try {
      await saveConfig(json);
      onReload();
    } catch (e) {
      setProbeError(String(e));
    } finally {
      // 保存后 reload 可能又回到「未找到配置」这个分支，不在这里解锁按钮
      // 就会永久卡在 disabled 上
      setSaving(false);
    }
  };

  if (error.kind === "not_found") {
    // 本机 ssh config 里已经有服务器可用时，直接列出来一键起会话 —— 比让用户
    // 面对一屏 REPLACE_WITH_* 占位符强得多。
    const canImport = sshHosts.length > 0;
    const busy = probingAlias !== null || saving;
    return (
      <div className="config-error">
        <p className="error-title">
          {canImport ? "选择一台服务器开始" : "未找到配置文件"}
        </p>
        {canImport && (
          <>
            <p className="muted">
              检测到本机 <code>~/.ssh/config</code> 里的主机，点一个会先去探测那台
              机器上装了哪些 agent：
            </p>
            <div className="row">
              {sshHosts.map((h) => (
                <button
                  key={h.alias}
                  className="primary"
                  title={describeSshHost(h)}
                  disabled={busy}
                  onClick={() => void probe(h.alias)}
                >
                  {probingAlias === h.alias ? `探测 ${h.alias} 中…` : `用 ${h.alias} 开始`}
                </button>
              ))}
            </div>

            {probingAlias && (
              <p className="muted">
                正在探测 {probingAlias}…（没装 agent 或连不上都要等几秒）
              </p>
            )}

            {probeError && <p className="error">探测失败：{probeError}</p>}

            {result && result.agents.length > 0 && (
              <>
                <p className="muted">
                  在 <code>{result.alias}</code> 上找到这些 agent，确认后写进配置：
                </p>
                <div className="row">
                  {result.agents.map((a) => (
                    <span className="agent-head" key={a.id}>
                      <AgentIcon agent={a.id} size={18} />
                      {a.label}
                    </span>
                  ))}
                </div>
                <div className="row">
                  <button className="primary" disabled={saving} onClick={() => void confirm()}>
                    {saving ? "保存中…" : `用这几个开始（${result.agents.length}）`}
                  </button>
                  <button disabled={saving} onClick={() => setResult(null)}>
                    换一台
                  </button>
                </div>
              </>
            )}

            {result && result.agents.length === 0 && (
              <>
                <p className="error">
                  <code>{result.alias}</code> 上没有探测到已知 agent。
                </p>
                <p className="muted">
                  可能是那台机器确实没装，也可能是连不上（探测走的是一次性 ssh，
                  只认免密登录）。换一台，或者在下面生成模板自己填。
                </p>
                <div className="row">
                  <button onClick={() => setResult(null)}>换一台</button>
                </div>
              </>
            )}

            <p className="muted">
              没有你要的？也可以生成一份模板自己填（生成后会自动打开编辑面板）。
            </p>
          </>
        )}
        <p className="muted">已查找以下位置：</p>
        <ul className="path-list">
          {error.searched.map((p) => (
            <li key={p}>
              <code>{p}</code>
            </li>
          ))}
        </ul>
        <div className="row">
          <button className={canImport ? "" : "primary"} onClick={onGenerate}>
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
