/**
 * Calls into the Rust side (src-tauri/src/commands.rs), or a stand-in when the page runs in a plain
 * browser (`npm run dev`, then open http://localhost:1441/?sample=1). The stand-in keeps settings in
 * memory and answers from made-up data (one project of plain shapes with ?sample), so the screens
 * can be checked without Tauri. Nothing is read from or written to disk.
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

// ---- made-up preview data (no real project): a few folders of plain shapes

const PREVIEW_PROJECT = {
  id: "p000000000001",
  packId: "default",
  exists: true,
  mode: "batch",
  reason: "closed",
  lock: "none",
  path: "C:\\Projects\\ToyTown",
  name: "ToyTown",
  unityVersion: "6000.0.1f1",
  unityRevision: null,
  urp: true,
  pipelinePackage: true,
  branch: "main",
  isGit: true,
};

const PREVIEW_NAMES: [string, string, string][] = [
  ["Buildings", "building", "Bakery Cafe Clinic Library Mill Station Tower Workshop Lighthouse Greenhouse"],
  ["Props", "prop", "Crate Barrel Lamp Bench Mushroom Hat Lantern Teapot Cart Mailbox Fountain Kite"],
  ["Characters", "character", "Robot Fox Owl Bunny Knight Chef"],
  ["Nature", "generic", "Tree Bush Rock Flower Log Cloud"],
];

function previewScan(): unknown {
  const prefabs: unknown[] = [];
  let n = 0;
  for (const [folder, kind, names] of PREVIEW_NAMES) {
    for (const name of names.split(" ")) {
      n++;
      prefabs.push({
        guid: n.toString(16).padStart(32, "0"),
        path: `Assets/Game/${folder}/${name}.prefab`,
        name,
        folder: `Assets/Game/${folder}`,
        class: "3d",
        kind,
        kindSource: kind === "character" ? "skinned" : kind === "generic" ? "default" : "rule",
        rule: kind === "building" || kind === "prop" ? `**/${folder}/*.prefab` : undefined,
        skinned: kind === "character",
        nested: 1,
        mtimeMs: 0,
      });
    }
  }
  for (const [name, cls] of [["MainMenu", "ui"], ["CoinPopup", "ui"], ["Sparkle", "fx"], ["Coin2D", "2d"], ["Spawner", "empty"]]) {
    n++;
    prefabs.push({
      guid: n.toString(16).padStart(32, "0"),
      path: `Assets/Game/Misc/${name}.prefab`,
      name,
      folder: "Assets/Game/Misc",
      class: cls,
      kind: "generic",
      kindSource: "default",
      skinned: false,
      nested: 0,
      mtimeMs: 0,
    });
  }
  const counts: Record<string, number> = {};
  const kinds: Record<string, number> = {};
  for (const p of prefabs as { class: string; kind: string }[]) {
    counts[p.class] = (counts[p.class] ?? 0) + 1;
    kinds[p.kind] = (kinds[p.kind] ?? 0) + 1;
  }
  return {
    schema: "unity-icon-studio.scan/1",
    project: PREVIEW_PROJECT.path,
    packId: "default",
    total: prefabs.length,
    counts,
    kinds,
    metas: 400,
    models: 30,
    elapsedMs: 42,
    cache: { metasRead: 0, metasCached: 400, prefabsRead: 0, prefabsCached: prefabs.length, usedCacheFile: true },
    prefabs,
  };
}

const PREVIEW_PACKS = [
  { id: "default", name: { en: "Casual thick outline" }, source: "builtin", extends: null, kindRules: 0, styleRefs: 0, dir: null, problem: null },
];

async function preview(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
  const sample = query.has("sample");
  switch (cmd) {
    case "project_list":
      return sample ? [PREVIEW_PROJECT] : [];
    case "project_probe":
      return PREVIEW_PROJECT;
    case "prefab_scan":
      await new Promise((resolve) => setTimeout(resolve, 300));
      return previewScan();
    case "pack_list":
      return PREVIEW_PACKS;
    case "hub_projects":
      return [];
    case "prefab_override":
      return args?.patch ?? {};
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
