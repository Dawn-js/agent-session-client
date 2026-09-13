import { useEffect, useRef, useState } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { ClipboardAddon } from "@xterm/addon-clipboard";
import { contextMenuAction, shouldCopySelection } from "./terminalClipboard";
import "@xterm/xterm/css/xterm.css";

export type ThemeName = "dark" | "light";

interface Props {
  onData: (data: string) => void;
  onResize: (cols: number, rows: number) => void;
  registerWriter: (write: (data: string) => void) => void;
  /**
   * 滚轮事件（true = 向上）。由上层直接下发 tmux 命令来滚动，不走终端按键 ——
   * 用户环境里 Ctrl+b 之类的按键到不了 tmux，而 tmux 侧本身是好的。
   */
  onScroll?: (up: boolean) => void;
  theme: ThemeName;
}

/**
 * 等宽字体栈。Cascadia Mono / Consolas 在 Windows 上比 xterm 默认的
 * `courier-new` 清晰得多 —— 后者笔画细、小字号下发虚，是"字体不清晰"的根因。
 */
const FONT_FAMILY =
  '"Cascadia Mono", "Cascadia Code", Consolas, "JetBrains Mono", ui-monospace, ' +
  "SFMono-Regular, Menlo, monospace";

/**
 * 两套配色都要对齐各自的 `--bg-panel`（深色 #171a23 / 浅色 #ffffff），
 * 并且 16 色 ANSI 全部按底色重定过。
 *
 * xterm 的默认调色板是给纯黑背景调的，直接用在深蓝灰面板上会整体发灰；
 * 深色盘原样搬到白底上则前景/背景对比度不够 —— 两套都不能省。
 */
const THEMES: Record<ThemeName, Record<string, string>> = {
  dark: {
    background: "#171a23",
    foreground: "#e6e8ef",
    cursor: "#5b8cff",
    cursorAccent: "#171a23",
    selectionBackground: "#33406b",
    black: "#2a2f3f",
    red: "#ff6b6b",
    green: "#4ecb8d",
    yellow: "#e0b341",
    blue: "#5b8cff",
    magenta: "#c58cff",
    cyan: "#4fc3d9",
    white: "#c8cede",
    brightBlack: "#7c86a0",
    brightRed: "#ff8a8a",
    brightGreen: "#6fe0a8",
    brightYellow: "#f0c862",
    brightBlue: "#8fb2ff",
    brightMagenta: "#d9a8ff",
    brightCyan: "#72d9ea",
    brightWhite: "#f2f4fa",
  },
  light: {
    background: "#ffffff",
    foreground: "#1b2030",
    cursor: "#2f6bff",
    cursorAccent: "#ffffff",
    selectionBackground: "#cfe0ff",
    black: "#5b6272",
    red: "#c0261a",
    green: "#0f7a56",
    yellow: "#8a5a00",
    blue: "#1f57e0",
    magenta: "#8b31c9",
    cyan: "#0b6d80",
    white: "#7a8296",
    brightBlack: "#98a2b3",
    brightRed: "#d92d20",
    brightGreen: "#12805c",
    brightYellow: "#a15c07",
    brightBlue: "#2f6bff",
    brightMagenta: "#a648e0",
    brightCyan: "#0d87a0",
    brightWhite: "#3a4152",
  },
};

/** resize 抖动时只认最后一次。每次 resize 后端都会注入一遍 tmux 重绘序列。 */
const RESIZE_DEBOUNCE_MS = 120;

export function Terminal({ onData, onResize, registerWriter, onScroll, theme }: Props) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<XTerm | null>(null);
  const searchRef = useRef<SearchAddon | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [query, setQuery] = useState("");

  useEffect(() => {
    const term = new XTerm({
      convertEol: false,
      scrollback: 5000,
      fontSize: 13.5,
      fontFamily: FONT_FAMILY,
      fontWeight: 400,
      fontWeightBold: 600,
      lineHeight: 1.32,
      letterSpacing: 0.2,
      cursorBlink: true,
      cursorStyle: "bar",
      theme: THEMES[theme],
    });
    termRef.current = term;
    const fit = new FitAddon();
    const search = new SearchAddon();
    term.loadAddon(fit);
    term.loadAddon(search);
    // tmux 的 set-clipboard on 会把复制结果发成 OSC 52，靠这个 addon 落到系统剪贴板
    term.loadAddon(new ClipboardAddon());
    term.loadAddon(
      new WebLinksAddon((_event, uri) => {
        // Tauri 的 webview 里 window.open 常被拦；打不开就把链接复制走，
        // 免得点了没反应（也省掉 opener 插件）
        const opened = window.open(uri, "_blank");
        if (!opened) void navigator.clipboard?.writeText(uri);
      }),
    );
    searchRef.current = search;
    term.open(hostRef.current!);
    fit.fit();

    // 滚轮：不让 xterm 本地滚，也不发给远端，而是由上层直接命令 tmux 滚
    // （返回 false = xterm 不处理这个事件）
    term.attachCustomWheelEventHandler((ev) => {
      if (!onScroll) return true;
      onScroll(ev.deltaY < 0);
      return false;
    });

    term.attachCustomKeyEventHandler((e) => {
      if (e.type === "keydown" && e.ctrlKey && e.shiftKey && e.key.toLowerCase() === "f") {
        setSearchOpen(true);
        return false; // 别让 xterm 把它当输入发出去
      }
      return true;
    });

    term.onKey(({ key, domEvent }) => {
      // 有选区时 Ctrl+C / Cmd+C 是复制，不把 ETX 发给远端。
      // 没有选区时不拦截；后端仍会按现有安全策略丢弃它。
      if (
        shouldCopySelection({
          key,
          domEventKey: domEvent.key,
          ctrlKey: domEvent.ctrlKey,
          metaKey: domEvent.metaKey,
          hasSelection: term.hasSelection(),
        })
      ) {
        domEvent.preventDefault();
        domEvent.stopPropagation();
        const selection = term.getSelection();
        if (selection) {
          const copy = navigator.clipboard?.writeText(selection);
          if (copy) {
            void copy.catch(() => {
              /* 剪贴板权限被拒绝时保留选区，不打断终端输入 */
            });
          }
        }
      }
    });
    term.onData(onData);
    registerWriter((data) => term.write(data));

    let resizeTimer: number | undefined;
    const ro = new ResizeObserver(() => {
      window.clearTimeout(resizeTimer);
      resizeTimer = window.setTimeout(() => {
        fit.fit();
        onResize(term.cols, term.rows);
      }, RESIZE_DEBOUNCE_MS);
    });
    ro.observe(hostRef.current!);

    return () => {
      window.clearTimeout(resizeTimer);
      ro.disconnect();
      term.dispose();
      termRef.current = null;
      searchRef.current = null;
      // xterm 的 dispose 不摘掉它自己插入的 DOM。不清空的话，下一次挂载
      // （StrictMode 双挂载、或任何 effect 重跑）会往同一个容器里再插一棵树，
      // 两棵树叠着渲染 —— 看起来就是"每个字都重复一遍"。
      hostRef.current?.replaceChildren();
    };
    // theme 不进依赖：换主题只是改配色，重建终端会丢掉整屏回滚历史
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [onData, onResize, registerWriter, onScroll]);

  useEffect(() => {
    if (termRef.current) termRef.current.options.theme = THEMES[theme];
  }, [theme]);

  const closeSearch = () => {
    setSearchOpen(false);
    setQuery("");
    searchRef.current?.clearDecorations();
    termRef.current?.focus();
  };

  const runSearch = (next: boolean) => {
    if (!query) return;
    if (next) searchRef.current?.findNext(query);
    else searchRef.current?.findPrevious(query);
  };

  return (
    <div className="term-wrap">
      <div
        ref={hostRef}
        className="terminal-host"
        // 文件面板拖过来的条目：把远端路径直接喂给远端 shell
        onDragOver={(e) => {
          // 不 preventDefault 浏览器就不认这是 drop 目标
          e.preventDefault();
          e.dataTransfer.dropEffect = "copy";
        }}
        onDrop={(e) => {
          e.preventDefault();
          const path = e.dataTransfer.getData("text/plain");
          if (path) onData(path);
        }}
        // 右键：有选区就复制，没有选区就粘贴。
        // 都是用户手势，WebView2 在这个前提下允许访问剪贴板。
        onContextMenu={(e) => {
          e.preventDefault();
          const action = contextMenuAction(termRef.current?.hasSelection() ?? false);
          if (action === "copy") {
            const selection = termRef.current?.getSelection() ?? "";
            const copy = selection ? navigator.clipboard?.writeText(selection) : undefined;
            if (copy) {
              void copy.catch(() => {
                /* 没有剪贴板权限就静默忽略，不打断输入 */
              });
            }
            return;
          }
          const paste = navigator.clipboard?.readText();
          if (paste) {
            void paste
              .then((text) => {
                if (text) onData(text);
              })
              .catch(() => {
                /* 没有剪贴板权限就静默忽略，不打断输入 */
              });
          }
        }}
      />

      {searchOpen && (
        <div className="term-search">
          <input
            autoFocus
            value={query}
            placeholder="搜索回滚缓冲"
            onChange={(e) => {
              setQuery(e.target.value);
              if (e.target.value) searchRef.current?.findNext(e.target.value);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") runSearch(!e.shiftKey);
              else if (e.key === "Escape") closeSearch();
            }}
          />
          <button title="上一个 (Shift+Enter)" onClick={() => runSearch(false)}>
            ↑
          </button>
          <button title="下一个 (Enter)" onClick={() => runSearch(true)}>
            ↓
          </button>
          <button title="关闭 (Esc)" onClick={closeSearch}>
            ×
          </button>
        </div>
      )}
    </div>
  );
}
