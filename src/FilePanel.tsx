import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { addTodo, loadTodos, newTodoId, removeTodo, saveTodos, toggleTodo, type Todo } from "./todos";

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
  /** 当前会话所在的 host；null 表示没有活动会话，文件页签不可用（待办不受影响） */
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
  const [tab, setTab] = useState<"files" | "todos">("files");
  const [view, setView] = useState<DirView | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [todos, setTodos] = useState<Todo[]>(loadTodos);
  const [draft, setDraft] = useState("");
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

  // 换 host（切会话）就重置文件视图；待办是全局的，不动
  useEffect(() => {
    setView(null);
    setError(null);
    if (host && tab === "files") void loadDir("~");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [host]);

  const updateTodos = (next: Todo[]) => {
    setTodos(next);
    saveTodos(next);
  };

  const submitTodo = () => {
    const next = addTodo(todos, draft, newTodoId());
    if (next === todos) return; // 空白输入
    updateTodos(next);
    setDraft("");
  };

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
          className={`files-tab${tab === "todos" ? " is-active" : ""}`}
          onClick={() => setTab("todos")}
        >
          待办
        </button>
      </div>

      {tab === "files" ? (
        !host ? (
          <p className="muted files-msg">开始一个会话后，这里会显示该主机的文件系统。</p>
        ) : (
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
        )
      ) : (
        <>
          <div className="files-bar">
            <input
              className="todo-input"
              value={draft}
              placeholder="添加待办，回车确认"
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submitTodo();
              }}
            />
            <button
              className="files-icon-btn"
              title="添加"
              disabled={!draft.trim()}
              onClick={submitTodo}
            >
              +
            </button>
          </div>

          {todos.length === 0 ? (
            <p className="muted files-msg">还没有待办。</p>
          ) : (
            <ul className="files-list">
              {todos.map((t) => (
                <li key={t.id}>
                  <div className="todo-row">
                    <input
                      type="checkbox"
                      checked={t.done}
                      onChange={() => updateTodos(toggleTodo(todos, t.id))}
                    />
                    <span className={`todo-text${t.done ? " is-done" : ""}`} title={t.text}>
                      {t.text}
                    </span>
                    <button
                      className="icon-btn"
                      title="删除"
                      onClick={() => updateTodos(removeTodo(todos, t.id))}
                    >
                      ×
                    </button>
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
