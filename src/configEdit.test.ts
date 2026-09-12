import { describe, expect, it } from "vitest";
import type { ConfigView } from "./config";
import {
  buildConfigJson,
  toEditableAgents,
  toEditableHosts,
} from "./configEdit";

const VIEW: ConfigView = {
  hosts: [
    { name: "main", host: "10.0.0.1", user: "ubuntu", extra_ssh_args: ["-p", "2222"] },
    { name: "bare", host: "10.0.0.2", user: null, extra_ssh_args: [] },
  ],
  agents: [{ id: "claude", label: "Claude Code", cmd: "claude" }],
  source_path: "/tmp/config.json",
  searched: [],
};

describe("toEditable*", () => {
  it("round-trips host fields into the form shape", () => {
    expect(toEditableHosts(VIEW)).toEqual([
      { name: "main", host: "10.0.0.1", user: "ubuntu", extra: "-p 2222" },
      { name: "bare", host: "10.0.0.2", user: "", extra: "" },
    ]);
  });

  it("copies agent fields verbatim", () => {
    expect(toEditableAgents(VIEW)).toEqual([{ id: "claude", label: "Claude Code", cmd: "claude" }]);
  });
});

describe("buildConfigJson", () => {
  it("produces JSON the backend parser accepts, preserving all fields", () => {
    const json = buildConfigJson(toEditableHosts(VIEW), toEditableAgents(VIEW));
    const parsed = JSON.parse(json);
    expect(parsed.hosts[0]).toEqual({
      name: "main",
      host: "10.0.0.1",
      user: "ubuntu",
      extra_ssh_args: ["-p", "2222"],
    });
    expect(parsed.agents[0]).toEqual({ id: "claude", label: "Claude Code", cmd: "claude" });
  });

  it("omits empty optional fields instead of writing nulls", () => {
    const json = buildConfigJson(
      [{ name: "h", host: "x", user: "  ", extra: "   " }],
      [{ id: "a", label: "A", cmd: "c" }],
    );
    const parsed = JSON.parse(json);
    expect(parsed.hosts[0]).toEqual({ name: "h", host: "x" });
    expect(JSON.stringify(parsed)).not.toContain("null");
  });

  it("trims values and keeps empty rows for the backend to reject", () => {
    const json = buildConfigJson(
      [{ name: "  ", host: " x ", user: "", extra: "" }],
      [],
    );
    const parsed = JSON.parse(json);
    expect(parsed.hosts[0].name).toBe("");
    expect(parsed.hosts[0].host).toBe("x");
    expect(parsed.agents).toEqual([]);
  });
});
