import { useEffect, useRef } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";

interface Props {
  onData: (data: string) => void;
  onResize: (cols: number, rows: number) => void;
  registerWriter: (write: (data: string) => void) => void;
}

/**
 * 等宽字体栈。Cascadia Mono / Consolas 在 Windows 上比 xterm 默认的
 * `courier-new` 清晰得多 —— 后者笔画细、小字号下发虚，是"字体不清晰"的根因。
 */
const FONT_FAMILY =
  '"Cascadia Mono", "Cascadia Code", Consolas, "JetBrains Mono", ui-monospace, ' +
  "SFMono-Regular, Menlo, monospace";

/**
 * 与 `--bg-panel` (#171a23) 对齐的配色。
 *
 * xterm 的默认调色板是给纯黑背景调的，直接用在深蓝灰面板上会整体发灰 ——
 * 16 色 ANSI 全部按这个背景重新定过，这是"对比度不高"的根因。
 */
const THEME = {
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
};

export function Terminal({ onData, onResize, registerWriter }: Props) {
  const hostRef = useRef<HTMLDivElement | null>(null);

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
      theme: THEME,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(hostRef.current!);
    fit.fit();

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
    };
  }, [onData, onResize, registerWriter]);

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
      // 右键粘贴。用 navigator.clipboard 而不是剪贴板插件：
      // 右键是用户手势，WebView2 在这个前提下允许读剪贴板。
      onContextMenu={(e) => {
        e.preventDefault();
        navigator.clipboard
          .readText()
          .then((text) => {
            if (text) onData(text);
          })
          .catch(() => {
            /* 没有剪贴板权限就静默忽略，不打断输入 */
          });
      }}
    />
  );
}
