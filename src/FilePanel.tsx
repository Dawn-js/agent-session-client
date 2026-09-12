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

interface Props {
  /** 当前会话所在的 host；null 表示没有活动会话，面板不可用 */
  host: string | null;
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

export function FilePanel({ host }: Props) {
  const [view, setView] = useState<DirView | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // 连点目录时请求会并发，只认最后一次发出的那个结果
  const seqRef = useRef(0);

  const load = useCallback(
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

  // 换 host（切会话）就回到该 host 的家目录
  useEffect(() => {
    setView(null);
    setError(null);
    if (host) void load("~");
  }, [host, load]);

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
      <h2 className="panel-title">服务器文件</h2>

      <div className="files-bar">
        <button
          className="files-icon-btn"
          title="上一级"
          disabled={!view || view.dir === "/"}
          onClick={() => void load(`${dir}/..`)}
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
          onClick={() => void load(dir)}
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
                  if (e.is_dir) void load(e.path);
                }}
              >
                <span className="file-name">{e.name}</span>
                {!e.is_dir && <span className="file-size">{formatSize(e.size)}</span>}
              </div>
            </li>
          ))}
        </ul>
      )}
    </aside>
  );
}
