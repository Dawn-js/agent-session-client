import { useEffect, useRef } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";

interface Props {
  onData: (data: string) => void;
  onResize: (cols: number, rows: number) => void;
  registerWriter: (write: (data: string) => void) => void;
}

export function Terminal({ onData, onResize, registerWriter }: Props) {
  const hostRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const term = new XTerm({ convertEol: false, scrollback: 5000, fontSize: 13 });
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

  return <div ref={hostRef} style={{ flex: 1, minHeight: 0 }} />;
}
