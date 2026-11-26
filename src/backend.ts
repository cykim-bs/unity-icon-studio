/**
 * Calls into the Rust side (src-tauri/src/commands.rs), or a stand-in when the page runs in a plain
 * browser (`npm run dev`, then open http://localhost:1441/?sample=1). The stand-in keeps settings in
 * memory and answers from made-up data, so the screens can be checked without Tauri.
 *
 * Preview query (browser only): ?sample=1 ?lang=ko|en ?theme=system|light|dark ?nav=projects|runs|packs|closed
 * ?body=hidden ?frame=<width>x<height>
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export const inTauri = typeof (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ !== "undefined";

const query = new URLSearchParams(location.search);
const previewTheme = query.get("theme");

/** Stands in for settings.json (dotted keys as the Rust side takes them). */
const PREVIEW_SETTINGS: Record<string, unknown> = {
  language: query.get("lang") ?? "auto",
  theme: previewTheme === "light" || previewTheme === "dark" ? previewTheme : "system",
};

function setPath(target: Record<string, unknown>, key: string, value: unknown): void {
  const parts = key.split(".");
  let node = target;
  for (const part of parts.slice(0, -1)) {
    if (typeof node[part] !== "object" || node[part] === null) node[part] = {};
    node = node[part] as Record<string, unknown>;
  }
  node[parts[parts.length - 1]] = value;
}

async function preview(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
  switch (cmd) {
    case "settings_get":
      return structuredClone(PREVIEW_SETTINGS);
    case "settings_set":
      for (const [key, value] of Object.entries((args?.patch as Record<string, unknown>) ?? {})) setPath(PREVIEW_SETTINGS, key, value);
      return structuredClone(PREVIEW_SETTINGS);
    case "settings_reset":
      for (const key of Object.keys(PREVIEW_SETTINGS)) if (key !== "projects") delete PREVIEW_SETTINGS[key];
      return structuredClone(PREVIEW_SETTINGS);
    case "app_info":
      return {
        version: "0.0.0-preview",
        coreVersion: "0.0.0",
        home: "(preview)",
        homeOverride: false,
        settingsFile: "(preview)\\settings.json",
        settingsProblem: null,
        dev: true,
      };
    default:
      return null;
  }
}

export function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return (inTauri ? invoke<T>(cmd, args) : preview(cmd, args)) as Promise<T>;
}

export async function on<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  if (!inTauri) return () => {};
  return listen<T>(event, (e) => handler(e.payload));
}
