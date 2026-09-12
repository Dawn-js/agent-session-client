import { useState } from "react";
import {
  probeAgents,
  saveConfig,
  type AgentView,
  type ConfigError,
  type ConfigView,
} from "./config";
import {
  buildConfigJson,
  mergeDiscoveredAgents,
  toEditableAgents,
  toEditableHosts,
  type EditableAgent,
  type EditableHost,
} from "./configEdit";

type InvalidError = Extract<ConfigError, { kind: "invalid" }>;

function isInvalidError(value: unknown): value is InvalidError {
  return (
    typeof value === "object" &&
    value !== null &&
    (value as { kind?: unknown }).kind === "invalid"
  );
}

export function SettingsModal({
  config,
  onClose,
  onSaved,
}: {
  config: ConfigView;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [hosts, setHosts] = useState<EditableHost[]>(() => toEditableHosts(config));
  const [agents, setAgents] = useState<EditableAgent[]>(() => toEditableAgents(config));
  const [errors, setErrors] = useState<string[] | null>(null);
  const [saving, setSaving] = useState(false);
  // 探测：只能用**已保存**的主机名 —— 后端是按已加载配置解析 host→target 的，
  // 面板里改了还没保存的主机它不认识。
  const [probeHost, setProbeHost] = useState(() => config.hosts[0]?.name ?? "");
  const [probing, setProbing] = useState(false);
  const [probeError, setProbeError] = useState<string | null>(null);
  const [probeNote, setProbeNote] = useState<string | null>(null);
  const [discovered, setDiscovered] = useState<AgentView[]>([]);

  const patchHost = (i: number, patch: Partial<EditableHost>) => {
    setHosts((prev) => prev.map((h, j) => (j === i ? { ...h, ...patch } : h)));
  };
  const patchAgent = (i: number, patch: Partial<EditableAgent>) => {
    setAgents((prev) => prev.map((a, j) => (j === i ? { ...a, ...patch } : a)));
  };

  const probe = async () => {
    setProbing(true);
    setProbeError(null);
    setProbeNote(null);
    try {
      const found = await probeAgents(probeHost);
      // 已经在配置里的不列出来，免得用户重复加入
      const configured = new Set(agents.map((a) => a.id.trim()));
      const fresh = found.filter((a) => !configured.has(a.id.trim()));
      setDiscovered(fresh);
      if (found.length === 0) setProbeNote("该主机上没有探测到已知 agent");
      else if (fresh.length === 0) setProbeNote("探测到的 agent 都已经在配置里了");
    } catch (error) {
      setDiscovered([]);
      setProbeError(String(error));
    } finally {
      setProbing(false);
    }
  };

  const addDiscovered = (agent: AgentView) => {
    setAgents((prev) => mergeDiscoveredAgents(prev, [agent]));
    setDiscovered((prev) => prev.filter((a) => a.id !== agent.id));
  };

  const addAllDiscovered = () => {
    setAgents((prev) => mergeDiscoveredAgents(prev, discovered));
    setDiscovered([]);
  };

  const save = async () => {
    setSaving(true);
    setErrors(null);
    try {
      await saveConfig(buildConfigJson(hosts, agents));
      onSaved();
      onClose();
    } catch (error) {
      setErrors(
        isInvalidError(error) ? error.errors : [String(error)],
      );
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      {/* 点击遮罩关闭；点击面板内部不关闭 */}
      <div
        className="modal"
        role="dialog"
        aria-label="编辑配置"
        tabIndex={-1}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-head">
          <h2>编辑配置</h2>
          <button className="icon-btn" onClick={onClose} aria-label="关闭">
            ✕
          </button>
        </div>
        <p className="muted modal-note">
          保存到用户配置目录的 config.json，保存后立即生效。
        </p>

        <div className="modal-body">
          <section>
            <div className="section-head">
              <h3>主机</h3>
              <button
                className="small-btn"
                onClick={() => setHosts((prev) => [...prev, { name: "", host: "", user: "", extra: "" }])}
              >
                + 添加主机
              </button>
            </div>
            {hosts.map((h, i) => (
              <div className="edit-row" key={i}>
                <input
                  placeholder="名称"
                  value={h.name}
                  onChange={(e) => patchHost(i, { name: e.target.value })}
                />
                <input
                  placeholder="地址（IP 或主机名）"
                  value={h.host}
                  onChange={(e) => patchHost(i, { host: e.target.value })}
                />
                <input
                  placeholder="用户名（可选）"
                  value={h.user}
                  onChange={(e) => patchHost(i, { user: e.target.value })}
                />
                <input
                  className="wide"
                  placeholder="额外 ssh 参数（可选，空格分隔）"
                  value={h.extra}
                  onChange={(e) => patchHost(i, { extra: e.target.value })}
                />
                <button
                  className="icon-btn"
                  aria-label="删除主机"
                  onClick={() => setHosts((prev) => prev.filter((_, j) => j !== i))}
                >
                  🗑
                </button>
              </div>
            ))}
          </section>

          <section>
            <div className="section-head">
              <h3>Agent</h3>
              <button
                className="small-btn"
                onClick={() => setAgents((prev) => [...prev, { id: "", label: "", cmd: "" }])}
              >
                + 添加 Agent
              </button>
            </div>

            {/* 探测远端 PATH 上装了哪些已知 agent；加入只是写进下面的列表，保存才落盘 */}
            <div className="probe-row">
              <select
                value={probeHost}
                onChange={(e) => setProbeHost(e.target.value)}
                disabled={config.hosts.length === 0}
                aria-label="探测哪台主机"
              >
                {config.hosts.length === 0 ? (
                  <option value="">（先添加主机）</option>
                ) : (
                  config.hosts.map((h) => (
                    <option key={h.name} value={h.name}>
                      {h.name}
                    </option>
                  ))
                )}
              </select>
              <button
                className="small-btn"
                onClick={() => void probe()}
                disabled={probing || !probeHost}
              >
                {probing ? "探测中…" : "🔍 探测已装 agent"}
              </button>
            </div>
            {probeError && <p className="error">{probeError}</p>}
            {probeNote && <p className="muted">{probeNote}</p>}
            {discovered.length > 0 && (
              <div className="discovered">
                <div className="section-head">
                  <span className="muted">探测到 {discovered.length} 个未配置的 agent</span>
                  <button className="small-btn" onClick={addAllDiscovered}>
                    全部加入
                  </button>
                </div>
                {discovered.map((a) => (
                  <div className="discovered-row" key={a.id}>
                    <span className="discovered-id">{a.id}</span>
                    <span className="muted discovered-label">{a.label}</span>
                    <code className="discovered-cmd">{a.cmd}</code>
                    <button className="small-btn" onClick={() => addDiscovered(a)}>
                      加入
                    </button>
                  </div>
                ))}
              </div>
            )}

            {agents.map((a, i) => (
              <div className="edit-row" key={i}>
                <input
                  placeholder="标识（如 claude）"
                  value={a.id}
                  onChange={(e) => patchAgent(i, { id: e.target.value })}
                />
                <input
                  placeholder="显示名（如 Claude Code）"
                  value={a.label}
                  onChange={(e) => patchAgent(i, { label: e.target.value })}
                />
                <input
                  className="wide"
                  placeholder="远端启动命令（如 claude）"
                  value={a.cmd}
                  onChange={(e) => patchAgent(i, { cmd: e.target.value })}
                />
                <button
                  className="icon-btn"
                  aria-label="删除 Agent"
                  onClick={() => setAgents((prev) => prev.filter((_, j) => j !== i))}
                >
                  🗑
                </button>
              </div>
            ))}
          </section>

          {errors && (
            <ul className="error-list">
              {errors.map((e) => (
                <li key={e}>{e}</li>
              ))}
            </ul>
          )}
        </div>

        <div className="modal-foot">
          <button onClick={onClose}>取消</button>
          <button className="primary" onClick={() => void save()} disabled={saving}>
            {saving ? "保存中…" : "保存"}
          </button>
        </div>
      </div>
    </div>
  );
}
