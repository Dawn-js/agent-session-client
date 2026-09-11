import { describe, expect, it } from "vitest";
import { applyState, type Session } from "./sessions";

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
