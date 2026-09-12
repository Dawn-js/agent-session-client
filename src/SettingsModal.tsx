import { useState } from "react";
import { saveConfig, type ConfigError, type ConfigView } from "./config";import {
  buildConfigJson,
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

  const patchHost = (i: number, patch: Partial<EditableHost>) => {
    setHosts((prev) => prev.map((h, j) => (j === i ? { ...h, ...patch } : h)));
  };
  const patchAgent = (i: number, patch: Partial<EditableAgent>) => {
    setAgents((prev) => prev.map((a, j) => (j === i ? { ...a, ...patch } : a)));
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
