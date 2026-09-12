import { describe, expect, it } from "vitest";
import { contextMenuAction, shouldCopySelection } from "./terminalClipboard";

describe("shouldCopySelection", () => {
  it("copies Ctrl+C when terminal text is selected", () => {
    expect(
      shouldCopySelection({ key: "\u0003", domEventKey: "c", ctrlKey: true, hasSelection: true }),
    ).toBe(true);
  });

  it("does not copy Ctrl+C when there is no selection", () => {
    expect(shouldCopySelection({ key: "c", ctrlKey: true, hasSelection: false })).toBe(false);
  });

  it("supports the platform modifier without treating ordinary c as copy", () => {
    expect(shouldCopySelection({ key: "c", metaKey: true, hasSelection: true })).toBe(true);
    expect(shouldCopySelection({ key: "c", hasSelection: true })).toBe(false);
    expect(shouldCopySelection({ key: "v", ctrlKey: true, hasSelection: true })).toBe(false);
  });
});

describe("contextMenuAction", () => {
  it("copies the selection when one exists", () => {
    expect(contextMenuAction(true)).toBe("copy");
  });

  it("pastes when there is no selection", () => {
    expect(contextMenuAction(false)).toBe("paste");
  });
});
