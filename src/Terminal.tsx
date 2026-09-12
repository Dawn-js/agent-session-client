import { useEffect, useRef } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { contextMenuAction, shouldCopySelection } from "./terminalClipboard";
import "@xterm/xterm/css/xterm.css";

export type ThemeName = "dark" | "light";

interface Props {
  onData: (data: string) => void;
  onResize: (cols: number, rows: number) => void;
  registerWriter: (write: (data: string) => void) => void;
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

export function Terminal({ onData, onResize, registerWriter, theme }: Props) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<XTerm | null>(null);

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
    term.loadAddon(fit);
    term.open(hostRef.current!);
    fit.fit();

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

    const ro = new ResizeObserver(() => {
      fit.fit();
      onResize(term.cols, term.rows);
    });
    ro.observe(hostRef.current!);

    return () => {
      ro.disconnect();
      term.dispose();
      termRef.current = null;
    };
    // theme 不进依赖：换主题只是改配色，重建终端会丢掉整屏回滚历史
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [onData, onResize, registerWriter]);

  useEffect(() => {
    if (termRef.current) termRef.current.options.theme = THEMES[theme];
  }, [theme]);

  return (
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
  );
}
