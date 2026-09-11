import { describe, expect, it } from "vitest";
import { applyState, STATE_META, type Session, type SessionState } from "./sessions";

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

describe("STATE_META", () => {
  it("covers every session state with a label and tone", () => {
    const states: SessionState[] = ["connecting", "connected", "retrying", "exited", "closed"];
    for (const state of states) {
      expect(STATE_META[state].label.length).toBeGreaterThan(0);
      expect(STATE_META[state].tone.length).toBeGreaterThan(0);
    }
  });
});
