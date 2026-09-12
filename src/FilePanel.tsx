import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Entry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
}

interface DirView {
  /** 远端 pwd 解析出的规范绝对路径 */
  dir: string;
  entries: Entry[];
}

interface Skill {
  name: string;
  description: string;
}

interface Props {
  /** 当前会话所在的 host；null 表示没有活动会话，面板不可用 */
  host: string | null;
  /** 当前会话对应的 agent id，决定去哪找 skill（`~/.<agent>/skills`） */
  agent: string | null;
}

/** 拖到终端时携带的数据格式。用标准的 text/plain，终端区域直接读它。 */
const DRAG_MIME = "text/plain";

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  const units = ["K", "M", "G", "T"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)}${units[unit]}`;
}

export function FilePanel({ host, agent }: Props) {
  const [tab, setTab] = useState<"files" | "skills">("files");
  const [view, setView] = useState<DirView | null>(null);
  const [skills, setSkills] = useState<Skill[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // 连点目录时请求会并发，只认最后一次发出的那个结果
  const seqRef = useRef(0);

  const loadDir = useCallback(
    async (dir: string) => {
      if (!host) return;
      const seq = (seqRef.current += 1);
      setLoading(true);
      setError(null);
      try {
        const next = await invoke<DirView>("list_dir", { host, dir });
        if (seq === seqRef.current) setView(next);
      } catch (e) {
        if (seq === seqRef.current) {
          setView(null);
          setError(String(e));
        }
      } finally {
        if (seq === seqRef.current) setLoading(false);
      }
    },
    [host],
  );

  const loadSkills = useCallback(async () => {
    if (!host || !agent) return;
    const seq = (seqRef.current += 1);
    setLoading(true);
    setError(null);
    try {
      const next = await invoke<Skill[]>("list_skills", { host, agent });
      if (seq === seqRef.current) setSkills(next);
    } catch (e) {
      if (seq === seqRef.current) {
        setSkills(null);
        setError(String(e));
      }
    } finally {
      if (seq === seqRef.current) setLoading(false);
    }
  }, [host, agent]);

  // 换 host / agent（切会话）就重置；tab 切换由按钮自己负责
  useEffect(() => {
    setView(null);
    setSkills(null);
    setError(null);
    if (host && tab === "files") void loadDir("~");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [host, agent]);

  const openSkills = () => {
    setTab("skills");
    if (skills === null) void loadSkills();
  };

  if (!host) {
    return (
      <aside className="files">
        <h2 className="panel-title">服务器文件</h2>
        <p className="muted files-msg">开始一个会话后，这里会显示该主机的文件系统。</p>
      </aside>
    );
  }

  const dir = view?.dir ?? "~";

  return (
    <aside className="files">
      <div className="files-tabs">
        <button
          className={`files-tab${tab === "files" ? " is-active" : ""}`}
          onClick={() => {
            setTab("files");
            if (!view) void loadDir("~");
          }}
        >
          文件
        </button>
        <button
          className={`files-tab${tab === "skills" ? " is-active" : ""}`}
          onClick={openSkills}
        >
          技能
        </button>
      </div>

      {tab === "files" ? (
        <>
          <div className="files-bar">
            <button
              className="files-icon-btn"
              title="上一级"
              disabled={!view || view.dir === "/"}
              onClick={() => void loadDir(`${dir}/..`)}
            >
              ↑
            </button>
            <code className="files-cwd" title={dir}>
              {dir}
            </code>
            <button
              className="files-icon-btn"
              title="刷新"
              disabled={loading}
              onClick={() => void loadDir(dir)}
            >
              ⟳
            </button>
          </div>

          {error ? (
            <p className="files-msg error">{error}</p>
          ) : !view ? (
            <p className="muted files-msg">{loading ? "读取中…" : "—"}</p>
          ) : view.entries.length === 0 ? (
            <p className="muted files-msg">（空目录）</p>
          ) : (
            <ul className={`files-list${loading ? " is-loading" : ""}`}>
              {view.entries.map((e) => (
                <li key={e.path}>
                  <div
                    className={`file-row${e.is_dir ? " is-dir" : ""}`}
                    draggable
                    title={`${e.path}\n\n拖到右侧终端即可插入路径`}
                    onDragStart={(ev) => {
                      ev.dataTransfer.setData(DRAG_MIME, e.path);
                      ev.dataTransfer.effectAllowed = "copy";
                    }}
                    onClick={() => {
                      if (e.is_dir) void loadDir(e.path);
                    }}
                  >
                    <span className="file-name">{e.name}</span>
                    {!e.is_dir && <span className="file-size">{formatSize(e.size)}</span>}
                  </div>
                </li>
              ))}
            </ul>
          )}
        </>
      ) : (
        <>
          <div className="files-bar">
            <code className="files-cwd" title={`~/.${agent}/skills`}>
              ~/.{agent}/skills
            </code>
            <button
              className="files-icon-btn"
              title="刷新"
              disabled={loading}
              onClick={() => void loadSkills()}
            >
              ⟳
            </button>
          </div>

          {error ? (
            <p className="files-msg error">{error}</p>
          ) : !skills ? (
            <p className="muted files-msg">{loading ? "读取中…" : "—"}</p>
          ) : skills.length === 0 ? (
            <p className="muted files-msg">
              没找到 skill。可能是这个 agent 不把 skill 放在 <code>~/.{agent}/skills</code>。
            </p>
          ) : (
            <ul className={`files-list${loading ? " is-loading" : ""}`}>
              {skills.map((s) => (
                <li key={s.name}>
                  <div className="skill-row" title={s.description || s.name}>
                    <span className="skill-name">{s.name}</span>
                    {s.description && <span className="skill-desc">{s.description}</span>}
                  </div>
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </aside>
  );
}
