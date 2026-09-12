import { describe, expect, it } from "vitest";
import type { AgentView, ConfigView } from "./config";
import {
  bootstrapConfigJson,
  buildConfigJson,
  mergeDiscoveredAgents,
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

describe("mergeDiscoveredAgents", () => {
  const existing = [{ id: "claude", label: "Claude Code", cmd: "claude" }];
  const discovered: AgentView[] = [
    { id: "claude", label: "Claude Code", cmd: "claude" },
    { id: "hermes", label: "Hermes Agent", cmd: "hermes chat" },
  ];

  it("appends only agents that are not already configured", () => {
    expect(mergeDiscoveredAgents(existing, discovered)).toEqual([
      { id: "claude", label: "Claude Code", cmd: "claude" },
      { id: "hermes", label: "Hermes Agent", cmd: "hermes chat" },
    ]);
  });

  it("dedupes by trimmed id, so a blank row is not a wildcard", () => {
    const got = mergeDiscoveredAgents(
      [{ id: " new ", label: "New", cmd: "n" }],
      [{ id: "new", label: "New", cmd: "n" }],
    );
    expect(got).toHaveLength(1);
  });

  it("skips empty ids and never mutates the input", () => {
    const before = [...existing];
    const got = mergeDiscoveredAgents(existing, [{ id: "  ", label: "", cmd: "" }]);
    expect(got).toEqual(existing);
    expect(existing).toEqual(before);
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

describe("bootstrapConfigJson", () => {
  const agents: AgentView[] = [
    { id: "hermes", label: "Hermes Agent", cmd: "hermes chat" },
    { id: "dsh", label: "DeepSeek Harness", cmd: "dsh" },
  ];

  it("uses the alias as both name and host, leaving ssh to resolve the rest", () => {
    const parsed = JSON.parse(bootstrapConfigJson("main", agents));
    expect(parsed.hosts).toEqual([{ name: "main", host: "main" }]);
  });

  it("trims the alias", () => {
    const parsed = JSON.parse(bootstrapConfigJson("  main  ", agents));
    expect(parsed.hosts[0]).toEqual({ name: "main", host: "main" });
  });

  it("carries the registry agents verbatim so both lists are non-empty", () => {
    const parsed = JSON.parse(bootstrapConfigJson("main", agents));
    expect(parsed.agents).toEqual(agents);
    expect(parsed.hosts).toHaveLength(1);
    expect(parsed.agents.length).toBeGreaterThan(0);
  });
});
