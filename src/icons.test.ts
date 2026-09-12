import { describe, expect, it } from "vitest";
import { letterColor, matchAgentIcon } from "./icons";

describe("matchAgentIcon", () => {
  it("matches known agents by id or label, case-insensitively", () => {
    expect(matchAgentIcon("claude")).not.toBeNull();
    expect(matchAgentIcon("Claude Code")).not.toBeNull();
    expect(matchAgentIcon("dsh")).not.toBeNull();
    expect(matchAgentIcon("DeepSeek Harness")).not.toBeNull();
    expect(matchAgentIcon("hermes-chat-gpt")).not.toBeNull();
    expect(matchAgentIcon("MY-GEMINI")).not.toBeNull();
  });

  it("matches on either id or label", () => {
    expect(matchAgentIcon("gpt-assistant")).not.toBeNull();
    expect(matchAgentIcon("totally-unknown")).toBeNull();
  });

  it("returns null for unknown agents", () => {
    expect(matchAgentIcon("hermes")).toBeNull();
    expect(matchAgentIcon("")).toBeNull();
  });
});

describe("letterColor", () => {
  it("is stable for the same name", () => {
    expect(letterColor("hermes")).toBe(letterColor("hermes"));
  });

  it("returns a color for any input", () => {
    for (const name of ["a", "hermes", "x-1", ""]) {
      expect(letterColor(name)).toMatch(/^#/);
    }
  });
});
