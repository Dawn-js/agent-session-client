import { invoke } from "@tauri-apps/api/core";

export interface HostView {
  name: string;
  host: string;
}

export interface AgentView {
  id: string;
  label: string;
}

export interface ConfigView {
  hosts: HostView[];
  agents: AgentView[];
  source_path: string;
  searched: string[];
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
