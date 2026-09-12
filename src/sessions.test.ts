import { describe, expect, it } from "vitest";
import { applyState, removeSession, STATE_META, type Session, type SessionState } from "./sessions";

describe("applyState", () => {
  it("adds an unknown session", () => {
    expect(applyState([], "a", "connecting")).toEqual([{ id: "a", state: "connecting" }]);
  });

  it("replaces the state of a known session", () => {
    const before: Session[] = [{ id: "a", state: "connecting" }];
    expect(applyState(before, "a", "connected")).toEqual([{ id: "a", state: "connected" }]);
  });

  it("never mutates the input array", () => {
    const before: Session[] = [{ id: "a", state: "connecting" }];
    applyState(before, "a", "retrying");
    expect(before[0].state).toBe("connecting");
  });
});

describe("removeSession", () => {
  it("drops only the matching session", () => {
    const before: Session[] = [
      { id: "a", state: "connected" },
      { id: "b", state: "exited" },
    ];
    expect(removeSession(before, "a")).toEqual([{ id: "b", state: "exited" }]);
  });

  it("is a no-op for an unknown id and never mutates the input", () => {
    const before: Session[] = [{ id: "a", state: "connected" }];
    expect(removeSession(before, "zzz")).toEqual(before);
    removeSession(before, "a");
    expect(before).toHaveLength(1);
  });
});

describe("STATE_META", () => {
  it("covers every session state with a label and tone", () => {
    const states: SessionState[] = ["connecting", "connected", "retrying", "exited", "closed"];
    for (const state of states) {
      expect(STATE_META[state].label.length).toBeGreaterThan(0);
      expect(STATE_META[state].tone.length).toBeGreaterThan(0);
    }
  });
});
