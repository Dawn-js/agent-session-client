export interface CopyShortcutInput {
  key: string;
  domEventKey?: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  hasSelection: boolean;
}

export function shouldCopySelection({
  key,
  ctrlKey = false,
  metaKey = false,
  hasSelection,
  domEventKey,
}: CopyShortcutInput): boolean {
  const pressedKey = domEventKey ?? key;
  return hasSelection && (ctrlKey || metaKey) && pressedKey.toLowerCase() === "c";
}

export function contextMenuAction(hasSelection: boolean): "copy" | "paste" {
  return hasSelection ? "copy" : "paste";
}
