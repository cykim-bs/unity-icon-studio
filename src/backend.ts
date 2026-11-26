/**
 * Calls into the Rust side (src-tauri/src/commands.rs), or a stand-in when the page runs in a plain
 * browser (`npm run dev`, then open http://localhost:1441/?sample=1). The stand-in keeps settings in
 * memory and answers from made-up data (one project of plain shapes with ?sample), so the screens
 * can be checked without Tauri. Nothing is read from or written to disk.
 *
 * Preview query (browser only): ?sample=1 ?lang=ko|en ?theme=system|light|dark ?nav=projects|runs|packs|closed
 * ?body=hidden ?frame=<width>x<height>
 */
import { Channel, invoke } from "@tauri-apps/api/core";
import type { CaptureEvent, CaptureReport, CopyInfo, GenerateReport, Readiness, RewriteReport } from "./types";
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
    case "recovery_status":
      return [];
    case "readiness":
      return PREVIEW_READY;
    case "images_today":
      return 0;
    case "prompt_rewrite":
      await new Promise((resolve) => setTimeout(resolve, 400));
      return previewRewrite(String(args?.note ?? ""));
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

const cli = (name: string) => ({
  found: true,
  path: `C:\Tools\${name}.exe`,
  source: "path",
  version: `${name} 1.0.0`,
  loggedIn: true,
  imageTool: name === "codex" ? true : null,
  enabled: true,
  problem: null,
});

const PREVIEW_READY: Readiness = {
  codex: cli("codex"),
  claude: cli("claude"),
  unityCli: { ...cli("unity"), loggedIn: null },
  codexApp: null,
  codexCopyNeeded: false,
  elapsedMs: 120,
};

/** A made-up rewrite: the note becomes one changed and one added sentence. */
function previewRewrite(note: string): RewriteReport {
  const color = "Color: bright, saturated pastel palette with a few strong accent colors; higher saturation and contrast than the capture; no haze, no fog, no bloom.";
  const empty = !note.trim();
  return {
    ok: true,
    note,
    cli: empty ? null : "claude",
    fallbackFrom: null,
    finalText: "",
    calls: empty ? [] : [{ cli: "claude", purpose: "rewrite", attempt: 1, elapsedMs: 4200, code: "ok" }],
    failure: null,
    choices: [],
    diff: {
      changed: !empty,
      outline: "keep",
      camera: "keep",
      summaryKo: empty ? "" : "(preview) The note was worked into the sentences.",
      rejected: empty ? [] : [{ part: "add a logo", reason: "forbidden:logo", by: "code" }],
      sentences: [
        { id: "HEADER.verbatim", block: "HEADER", locked: true, text: "Use this prompt exactly as written.", status: "same" },
        empty
          ? { id: "STYLE.color", block: "STYLE", locked: false, text: color, status: "same" }
          : { id: "STYLE.color", block: "STYLE", locked: false, text: "Color: cool winter palette; no haze, no fog, no bloom.", status: "replaced", before: color },
        ...(empty ? [] : [{ id: "SUBJECT.add1", block: "SUBJECT", locked: false, text: "A light layer of snow on the roof.", status: "added" as const }]),
      ],
    },
  };
}

async function previewGenerate(note: string): Promise<GenerateReport> {
  await new Promise((resolve) => setTimeout(resolve, 900));
  const rewrite = note.trim() ? previewRewrite(note) : null;
  return {
    ok: true,
    jobDir: "(preview)",
    finalText: "",
    rewrite,
    candidates: [
      {
        ok: true,
        raw: "preview:7",
        harvestStage: 2,
        width: 1254,
        height: 1254,
        hasAlpha: true,
        failure: null,
        call: { cli: "codex", purpose: "image", attempt: 1, elapsedMs: 52000, code: "ok" },
      },
    ],
    error: null,
    elapsedMs: 56000,
  };
}

/** The note worked into the fixed prompt; no picture. */
export function promptRewrite(packId: string, kind: string, note: string, cli: string): Promise<RewriteReport> {
  return call<RewriteReport>("prompt_rewrite", { packId, kind, note, cli });
}

/** A raw picture from the capture (prompt, rewrite when there is a note, Codex). */
export function generateRaw(packId: string, kind: string, capture: string, name: string, note: string, cli: string): Promise<GenerateReport> {
  if (!inTauri) return previewGenerate(note);
  return invoke<GenerateReport>("generate_raw", { packId, kind, capture, name, note, cli });
}

export function readiness(refresh: boolean): Promise<Readiness> {
  return call<Readiness>("readiness", { refresh });
}

/** Copies the desktop app's Codex into the tool's folder; progress in bytes. */
export function copyCodex(onProgress: (done: number, total: number) => void): Promise<CopyInfo> {
  if (!inTauri) return Promise.reject("preview");
  const channel = new Channel<{ done: number; total: number }>();
  channel.onmessage = (p) => onProgress(p.done, p.total);
  return invoke<CopyInfo>("codex_copy_from_app", { onEvent: channel });
}

/** A plain picture for the preview's made-up captures (no game art). */
function previewPicture(seed: number): string {
  const hue = (seed * 47) % 360;
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256"><path d="M48 120 L128 56 L208 120 V208 H48 Z" ` +
    `fill="hsl(${hue} 60% 62%)" stroke="hsl(${hue} 40% 30%)" stroke-width="6"/><rect x="108" y="150" width="40" height="58" ` +
    `fill="hsl(${(hue + 180) % 360} 50% 55%)"/></svg>`;
  return `data:image/svg+xml;base64,${btoa(svg)}`;
}

async function previewCapture(items: string[], onEvent: (e: CaptureEvent) => void): Promise<CaptureReport> {
  const total = items.length;
  const step = (stage: CaptureEvent["stage"], done: number, ms: number) => onEvent({ stage, message: "", done, total, elapsedMs: ms });
  step("probe", 0, 0);
  for (let i = 0; i < total; i++) {
    await new Promise((resolve) => setTimeout(resolve, 250));
    step("unity", i + 1, 300 + i * 250);
  }
  step("done", total, 300 + total * 250);
  return {
    runId: "20260101-000000-0000",
    mode: "batch",
    reason: "closed",
    project: PREVIEW_PROJECT.path,
    outDir: "(preview)",
    ok: true,
    error: null,
    items: items.map((prefab, i) => ({ id: `item${i}`, prefab, kind: "building", ok: true, output: `preview:${i}`, coverage: 0.4 })),
    failed: 0,
    exitCode: 0,
    pending: false,
    logTail: [],
    unityMs: total * 250,
    elapsedMs: 300 + total * 250,
  };
}

/** Captures `items` of project `id`; steps come to `onEvent`. */
export function captureOnly(id: string, items: string[], onEvent: (e: CaptureEvent) => void): Promise<CaptureReport> {
  if (!inTauri) return previewCapture(items, onEvent);
  const channel = new Channel<CaptureEvent>();
  channel.onmessage = onEvent;
  return invoke<CaptureReport>("capture_only", { id, items, purpose: "capture", onEvent: channel });
}

/** A capture PNG as a data: URL (the preview draws a plain shape). */
export function captureImage(path: string): Promise<string> {
  if (!inTauri) return Promise.resolve(previewPicture(Number(path.split(":")[1] ?? 0) + 1));
  return invoke<string>("capture_image", { path });
}

export function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return (inTauri ? invoke<T>(cmd, args) : preview(cmd, args)) as Promise<T>;
}

export async function on<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  if (!inTauri) return () => {};
  return listen<T>(event, (e) => handler(e.payload));
}
