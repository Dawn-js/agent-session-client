import { invoke } from "@tauri-apps/api/core";

export interface HostView {
  name: string;
  host: string;
  user: string | null;
  extra_ssh_args: string[];
}

export interface AgentView {
  id: string;
  label: string;
  cmd: string;
}

export interface ConfigView {
  hosts: HostView[];
  agents: AgentView[];
  source_path: string;
  searched: string[];
}

/**
 * A `Host` alias found in this machine's own `~/.ssh/config`.
 *
 * Only `alias` is used to connect (the system ssh resolves the rest); the other
 * fields are display-only metadata for the first-run picker.
 */
export interface SshHostView {
  alias: string;
  host_name: string | null;
  user: string | null;
  port: number | null;
  proxy_jump: string | null;
}

/** Structured config-load failure returned by the Rust `load_config` command. */
export type ConfigError =
  | { kind: "not_found"; searched: string[] }
  | { kind: "invalid"; path: string; errors: string[] };

export function isConfigError(value: unknown): value is ConfigError {
  if (typeof value !== "object" || value === null || !("kind" in value)) {
    return false;
  }
  const kind = (value as { kind: unknown }).kind;
  return kind === "not_found" || kind === "invalid";
}

/**
 * Ask the backend to discover and load a config.
 *
 * `overridePath` is optional: leave it undefined to let the backend search its
 * standard locations (user config dir, next to the executable, cwd, bundled
 * example). Only pass a value when explicitly pinning a path.
 */
export async function loadConfig(overridePath?: string): Promise<ConfigView> {
  return invoke<ConfigView>("load_config", { path: overridePath ?? null });
}

/** Create the first-run template in the user config dir; returns its path. */
export async function writeExampleConfig(): Promise<string> {
  return invoke<string>("write_example_config");
}

/**
 * Probe a host for installed known agent CLIs. Returns candidates only —
 * nothing is written to the config until the user adds them in the editor.
 */
export async function probeAgents(host: string): Promise<AgentView[]> {
  return invoke<AgentView[]>("probe_agents", { host });
}

/**
 * Validate and save the edited config JSON to the user config dir.
 * Rejects with the backend's structured `{ kind: "invalid", errors }` payload
 * when a field fails validation, so the editor can point at the offending row.
 */
export async function saveConfig(json: string): Promise<string> {
  return invoke<string>("save_config", { json });
}

/**
 * List `Host` aliases from this machine's `~/.ssh/config`, so the first run can
 * offer servers the user already has instead of a form to fill in.
 * An empty list (no ssh config) is a normal result, not an error.
 */
export async function listSshHosts(): Promise<SshHostView[]> {
  return invoke<SshHostView[]>("list_ssh_hosts");
}

/** Known agent templates from the backend registry — the same source `probe_agents` uses. */
export async function knownAgents(): Promise<AgentView[]> {
  return invoke<AgentView[]>("known_agents");
}
