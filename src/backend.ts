/**
 * Calls into the Rust side (src-tauri/src/commands.rs), or a stand-in when the page runs in a plain
 * browser (`npm run dev`, then open http://localhost:1441/?sample=1). The stand-in keeps settings in
 * memory and answers from made-up data (one project of plain shapes with ?sample), so the screens
 * can be checked without Tauri. Nothing is read from or written to disk.
 *
 * Preview query (browser only): ?sample=1 ?lang=ko|en ?theme=system|light|dark ?nav=projects|runs|packs|closed
 * ?body=hidden ?frame=<width>x<height> ?thumbs (made-up grid thumbnails) ?fail=limit|rewrite|capture (a failing run)
 * ?pick=<n> (pick the n-th prefab) ?empty (no project, for the empty state)
 */
import { Channel, invoke } from "@tauri-apps/api/core";
import type { Assembled, CameraView, CaptureEvent, CaptureReport, CheckItem, CheckReport, CopyInfo, ExportReport, GenerateReport, JobDetail, JobSummary, PostJobReport, Readiness, RewriteReport, RunArgs, RunEvent, RunItem, RunReport } from "./types";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export const inTauri = typeof (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ !== "undefined";

const query = new URLSearchParams(location.search);
const previewTheme = query.get("theme");

/** Stands in for settings.json (dotted keys as the Rust side takes them). */
const PREVIEW_SETTINGS: Record<string, unknown> = {
  language: query.get("lang") ?? "auto",
  theme: previewTheme === "light" || previewTheme === "dark" ? previewTheme : "system",
  exportDir: query.has("sample") ? "C:\\Exports" : null,
  exportSizes: [256],
  exportOriginal: true,
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
      return sample && !query.has("empty") ? [PREVIEW_PROJECT] : [];
    case "project_set_pack":
      return { ...PREVIEW_PROJECT, packId: args?.packId ?? "default" };
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
    case "prefab_override": {
      const guid = String(args?.guid ?? "");
      const next: Record<string, unknown> = { ...(previewOverrides[guid] ?? {}) };
      for (const [k, v] of Object.entries((args?.patch as Record<string, unknown>) ?? {})) {
        if (v === null) delete next[k];
        else next[k] = v;
      }
      previewOverrides[guid] = next;
      return next;
    }
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
export function promptRewrite(packId: string, kind: string, note: string, cli: string, camera: Record<string, unknown> | null = null): Promise<RewriteReport> {
  return call<RewriteReport>("prompt_rewrite", { packId, kind, note, cli, camera });
}

/** Made-up per-prefab camera choices of the browser preview, by guid. */
const previewOverrides: Record<string, Record<string, unknown>> = {};

/** The kind presets of the default pack (browser preview only). */
const PREVIEW_PRESETS: Record<string, Record<string, unknown>> = {
  building: { front: "+z", yaw: 28, pitch: 30, projection: "orthographic", fov: 20 },
  prop: { front: "+z", yaw: 25, pitch: 18, projection: "orthographic", fov: 20 },
  character: { front: "+z", yaw: 20, pitch: 10, projection: "orthographic", fov: 20 },
  generic: { front: "+z", yaw: 28, pitch: 30, projection: "orthographic", fov: 20 },
};

/** The camera of one prefab: preset, rule, a person's values and the values a capture uses. */
export function prefabCamera(id: string, kind: string, rule: string | null, guid: string | null): Promise<CameraView> {
  if (inTauri) return invoke<CameraView>("prefab_camera", { id, kind, rule, guid });
  const preset = { ...(PREVIEW_PRESETS[kind] ?? PREVIEW_PRESETS.generic) };
  const person = (guid && previewOverrides[guid]) || null;
  const effective: Record<string, unknown> = { ...preset };
  if (person) {
    for (const k of ["front", "rotation", "pitch", "projection", "fov"]) if (person[k] !== undefined) effective[k] = person[k];
    if (typeof person.yaw === "number") effective.yaw = Math.abs(person.yaw) * Math.sign((preset.yaw as number) || 1);
    if (typeof person.yawSign === "number") effective.yaw = Math.abs(effective.yaw as number) * (person.yawSign < 0 ? -1 : 1);
  }
  return Promise.resolve({ preset, rule: { ...preset }, person: person as CameraView["person"], effective });
}

/** A raw picture from the capture (prompt, rewrite when there is a note, Codex). */
export function generateRaw(packId: string, kind: string, capture: string, name: string, note: string, cli: string): Promise<GenerateReport> {
  if (!inTauri) return previewGenerate(note);
  return invoke<GenerateReport>("generate_raw", { packId, kind, capture, name, note, cli });
}

/** Post-processing of a candidate again with other options (no new picture). */
export function postRerun(job: string, cand: number, options: Record<string, unknown>): Promise<PostJobReport> {
  if (!inTauri) return previewPost(options);
  return invoke<PostJobReport>("post_rerun", { job, cand, options });
}

/** Copies the finished icons out of the job (the folder from the settings when `folder` is null). */
export function exportIcons(job: string, cand: number, sizes: number[], name: string, folder: string | null, rule: string | null = null): Promise<ExportReport> {
  if (!inTauri) {
    const base = rule || `icon_${name}`;
    const dir = folder ?? "C:\\Exports";
    const list = [...new Set([...(sizes.length ? sizes : [256]), ...(PREVIEW_SETTINGS.exportOriginal === false ? [] : [1024])])].sort((a, b) => a - b);
    const files = list.map((size, i) => ({ size, path: `${dir}\\${base}${i ? `_${size}` : ""}.png` }));
    return Promise.resolve({ ok: true, folder: dir, files, error: null });
  }
  return invoke<ExportReport>("export", { job, cand, sizes, name, folder, rule });
}

function previewItem(id: string, group: string, value: number, min: number | null, max: number | null): CheckItem {
  return { id, group, value, min, max, pass: (min === null || value >= min) && (max === null || value <= max) };
}

/** Made-up checks for the browser preview. */
async function previewPost(options: Record<string, unknown>, cand = 1): Promise<PostJobReport> {
  await new Promise((resolve) => setTimeout(resolve, 300));
  const stroke = options.outline === "none" ? 0 : 3.7;
  const items = [
    previewItem("cornerAlpha", "frame", 0, null, 0),
    previewItem("size", "frame", 0.88, 0.82, 0.995),
    previewItem("fill", "frame", 0.51, 0.38, 0.72),
    previewItem("largestPart", "frame", 1, 0.98, null),
    ...(options.outline === "none" ? [] : [previewItem("outlineWidth", "outline", stroke, 3, 5), previewItem("outlineUniform", "outline", 0.99, 0.75, null)]),
    previewItem("saturation", "color", cand === 2 ? 0.66 : 0.52, 0.35, 0.62),
    previewItem("value", "color", 0.86, 0.74, 0.96),
  ];
  const report: CheckReport = { pass: items.every((i) => i.pass), failed: items.filter((i) => !i.pass).map((i) => i.id), outlineOnly: false, items, metrics: { outlinePx: stroke, outlineColor: "#753719" } };
  const gen = [previewItem("iou", "generate", 0.88, 0.8, null), previewItem("nativeAlpha", "generate", 1, 1, null)];
  // Keep-the-capture pictures: made-up fidelity measures and one review (`?fail=fidelity` fails it)
  const strict = options.fidelity !== "stylized";
  const failFid = query.get("fail") === "fidelity" && cand === 1;
  const fidItems = [
    previewItem("fidelityProfile", "fidelity", failFid ? 0.031 : 0.006, null, 0.012),
    previewItem("deltaSaturation", "fidelity", 0.04, -0.05, 0.15),
    previewItem("deltaValue", "fidelity", 0.03, -0.08, 0.15),
    previewItem("foreignBlobs", "fidelity", failFid ? 3 : 0, null, 2),
    previewItem("outsideParts", "fidelity", 0, null, 1),
  ];
  const fidReport: CheckReport = { pass: fidItems.every((i) => i.pass), failed: fidItems.filter((i) => !i.pass).map((i) => i.id), outlineOnly: false, items: fidItems, metrics: { outlinePx: 0, outlineColor: "" } };
  const review = {
    cli: "codex",
    ok: true,
    pass: !failFid,
    failing: failFid ? [{ kind: "added", what: "(preview) sparkles", detail: "three small stars next to the roof", severity: "major" }] : [],
    notes: [{ kind: "pattern", what: "(preview) roof tiles", detail: "fewer tile rows than the capture", severity: "minor" }],
    summary: failFid ? "(preview) Added decorations." : "(preview) Same object, repainted.",
    failure: null,
  };
  const fidelity = strict
    ? {
        fidelityMode: "strict",
        fidelity: { metrics: { profile: fidItems[0].value, foreignBlobs: fidItems[3].value, foreign: 0, outsideParts: 0, outside: 0, missing: 0 }, report: fidReport, suspect: failFid, captureColor: [0.5, 0.8] as [number, number], generatedColor: [0.54, 0.83] as [number, number], map: null },
        reviews: [review],
      }
    : { fidelityMode: "stylized", fidelity: null, reviews: [] };
  return {
    ...fidelity,
    ok: true,
    jobDir: "(preview)",
    candidate: 1,
    kind: "building",
    master: "preview:1",
    options: { outline: String(options.outline ?? "code"), background: String(options.background ?? "auto") },
    post: { alphaSource: "native", removedParts: 0, layout: { strokePx: stroke ? 4.2 : 0 } },
    generated: { report: { pass: true, failed: [], outlineOnly: false, items: gen, metrics: { outlinePx: 0, outlineColor: "" } }, iou: 0.88, modelOutline: false, nativeAlpha: true },
    sizes: [{ size: 256, path: "preview:2", checks: report }],
    pass: report.pass && (!strict || (fidReport.pass && review.pass)),
    elapsedMs: 300,
  };
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
export function captureOnly(id: string, items: string[], onEvent: (e: CaptureEvent) => void, purpose: "capture" | "thumb" = "capture"): Promise<CaptureReport> {
  if (!inTauri) return previewCapture(items, onEvent);
  const channel = new Channel<CaptureEvent>();
  channel.onmessage = onEvent;
  return invoke<CaptureReport>("capture_only", { id, items, purpose, onEvent: channel });
}

/** The camera values a capture picture was taken with (null: unknown, e.g. the browser preview). */
export function captureValues(path: string): Promise<Record<string, unknown> | null> {
  if (!inTauri) return Promise.resolve(null);
  return invoke<Record<string, unknown> | null>("capture_values", { path });
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

// ---------------------------------------------------------------- whole runs (pipeline), jobs, thumbnails

/** Made-up jobs the browser preview's runs leave (newest first). */
const previewJobs: { summary: JobSummary; detail: JobDetail }[] = [];
let previewStop = false;

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** A whole run in the browser preview: made-up steps and checks. `?fail=limit|rewrite|capture` shows a failure. */
async function previewRun(args: RunArgs, onEvent: (e: RunEvent) => void): Promise<RunReport> {
  previewStop = false;
  const runId = "20260101-000000-0001";
  const total = args.items.length;
  const started = Date.now();
  const fail = query.get("fail");
  const emit = (stage: string, state: string, item: string | null, index: number, message = "") =>
    onEvent({ runId, stage, state, item, index, total, message, elapsedMs: Date.now() - started });
  const need = args.items.filter((i) => !i.capture);
  if (need.length) {
    emit("capture", "start", null, 0);
    for (let i = 0; i < need.length; i++) {
      await sleep(300);
      emit("capture", "progress", null, i + 1, `unity:${i + 1}/${need.length}`);
    }
    emit("capture", fail === "capture" ? "fail" : "ok", null, 0);
  } else emit("capture", "skip", null, 0);
  const items: RunItem[] = [];
  let halt: string | null = null;
  for (const [n, it] of args.items.entries()) {
    const index = n + 1;
    const name = it.asset.split("/").pop()!.replace(/\.prefab$/, "");
    const base: RunItem = {
      asset: it.asset,
      name,
      kind: it.kind ?? "building",
      capture: it.capture ?? `preview:${n}`,
      jobDir: `(preview)\\jobs\\${runId}-${name}`,
      state: "ok",
      stage: null,
      error: null,
      rewrite: null,
      candidates: [],
      best: null,
      pass: false,
      outlineKept: false,
      export: null,
      elapsedMs: 0,
    };
    if (fail === "capture") {
      items.push({ ...base, state: "failed", stage: "capture", error: "blocked:open" });
      continue;
    }
    if (halt || previewStop) {
      items.push({ ...base, state: previewStop ? "cancelled" : "stopped", error: halt });
      continue;
    }
    emit("prompt", "start", it.asset, index);
    await sleep(80);
    emit("prompt", "ok", it.asset, index);
    if (args.note.trim()) {
      emit("rewrite", "start", it.asset, index);
      await sleep(500);
      if (fail === "rewrite") {
        emit("rewrite", "fail", it.asset, index);
        const rw = previewRewrite(args.note);
        rw.ok = false;
        rw.failure = { kind: "bad_json", cli: args.cli, message: "(preview) the answer was not JSON", exitCode: 0 };
        rw.choices = ["otherCli", "withoutNote", "cancel"];
        items.push({ ...base, state: "failed", stage: "rewrite", error: "rewrite:bad_json:(preview)", rewrite: rw });
        continue;
      }
      emit("rewrite", "ok", it.asset, index);
      base.rewrite = previewRewrite(args.note);
    } else {
      emit("rewrite", "skip", it.asset, index);
      base.rewrite = previewRewrite("");
    }
    for (let c = 1; c <= Math.max(1, args.candidates); c++) {
      emit("generate", "start", it.asset, index);
      await sleep(1200);
      if (previewStop) break;
      if (fail === "limit") {
        const failure = { kind: "limit", cli: "codex", message: "(preview) usage limit reached", exitCode: 1 };
        emit("generate", "fail", it.asset, index, "limit");
        base.candidates.push({ cand: c, ok: false, raw: null, failure, post: null, postError: null, passed: 0, checked: 0 });
        halt = "limit";
        break;
      }
      emit("generate", "ok", it.asset, index, String(c));
      emit("post", "start", it.asset, index, String(c));
      const post = await previewPost({ outline: args.outline, background: args.background, fidelity: args.fidelity ?? "strict" }, c);
      emit("post", "ok", it.asset, index, String(c));
      const items2 = post.sizes[0].checks.items;
      emit("check", post.pass ? "ok" : "fail", it.asset, index);
      base.candidates.push({ cand: c, ok: true, raw: `preview:${10 + c}`, failure: null, post, postError: null, passed: items2.filter((i) => i.pass).length, checked: items2.length });
    }
    const posted = base.candidates.filter((c) => c.post);
    const best = posted.filter((c) => c.post!.pass).sort((a, b) => b.passed - a.passed)[0] ?? posted[0];
    base.best = best?.cand ?? null;
    base.pass = !!best?.post?.pass;
    if (!best) {
      base.state = previewStop ? "cancelled" : "failed";
      base.stage = "generate";
      base.error = halt ? `image:${halt}` : null;
    }
    base.elapsedMs = Date.now() - started;
    items.push(base);
    if (best) {
      const summary: JobSummary = {
        dir: base.jobDir!,
        at: runId.slice(0, 15),
        name,
        kind: base.kind,
        packId: "default",
        note: args.note,
        project: PREVIEW_PROJECT.path,
        projectName: PREVIEW_PROJECT.name,
        asset: it.asset,
        ok: true,
        state: "ok",
        pass: base.pass,
        failedStage: null,
        error: null,
        candidates: base.candidates.map((c) => c.cand),
        posted: posted.map((c) => c.cand),
        best: base.best,
        icon: "preview:2",
        exported: false,
      };
      const detail: JobDetail = {
        dir: base.jobDir!,
        job: { name, kind: base.kind, note: args.note, asset: it.asset, project: PREVIEW_PROJECT.path, best: base.best },
        posts: posted.map((c) => c.post!),
        diff: base.rewrite?.diff ?? null,
        capture: base.capture,
        export: null,
      };
      previewJobs.unshift({ summary, detail });
    }
  }
  const report: RunReport = {
    schema: "unity-icon-studio.run/1",
    runId,
    project: PREVIEW_PROJECT.path,
    ok: items.every((i) => i.state === "ok"),
    cancelled: previewStop,
    stopped: halt,
    error: fail === "capture" ? "blocked:open" : null,
    capture: need.length ? { runId, mode: "batch", reason: "closed", ok: fail !== "capture", error: fail === "capture" ? "blocked:open" : null, failed: 0, pending: false, elapsedMs: 300 * need.length } : null,
    items,
    elapsedMs: Date.now() - started,
  };
  emit("done", report.ok ? "ok" : report.cancelled ? "cancel" : "fail", null, 0);
  return report;
}

/** A whole run: capture (items without a picture), then each item's prompt, rewrite, picture, finish and checks. */
export function runStart(args: RunArgs, onEvent: (e: RunEvent) => void): Promise<RunReport> {
  if (!inTauri) return previewRun(args, onEvent);
  const channel = new Channel<RunEvent>();
  channel.onmessage = onEvent;
  return invoke<RunReport>("run_start", { args, onEvent: channel });
}

/** Stops the run in progress (a capture finishes its request first). */
export function runCancel(): Promise<boolean> {
  if (!inTauri) {
    previewStop = true;
    return Promise.resolve(true);
  }
  return invoke<boolean>("run_cancel");
}

export function jobsList(limit = 60): Promise<JobSummary[]> {
  if (!inTauri) return Promise.resolve(previewJobs.map((j) => j.summary));
  return invoke<JobSummary[]>("jobs_list", { limit });
}

export function jobGet(dir: string): Promise<JobDetail> {
  if (!inTauri) {
    const found = previewJobs.find((j) => j.detail.dir === dir);
    return found ? Promise.resolve(found.detail) : Promise.reject("noJob");
  }
  return invoke<JobDetail>("job_get", { job: dir });
}

/** Thumbnails of earlier captures, by prefab guid (data: URLs). */
export function prefabThumbs(id: string): Promise<Record<string, string>> {
  if (!inTauri) {
    if (!query.has("thumbs")) return Promise.resolve({});
    const out: Record<string, string> = {};
    for (let n = 1; n <= 12; n++) out[n.toString(16).padStart(32, "0")] = previewPicture(n);
    return Promise.resolve(out);
  }
  return invoke<Record<string, string>>("prefab_thumbs", { id });
}

/** The folder picker for the export folder (null: cancelled). */
export function exportPick(): Promise<string | null> {
  if (!inTauri) return Promise.resolve(null);
  return invoke<string | null>("export_pick");
}

/** The fixed prompt of a pack for a kind (no LLM). */
export function promptPreview(packId: string, kind: string, camera: Record<string, unknown> | null = null): Promise<Assembled> {
  if (!inTauri) return Promise.resolve(previewAssembled(kind, camera));
  return invoke<Assembled>("prompt_preview", { packId, kind, camera });
}

function previewAssembled(kind: string, camera: Record<string, unknown> | null = null): Assembled {
  const s = (id: string, locked: boolean, text: string) => ({ id, block: id.split(".")[0], locked, text, status: "same" as const });
  const sentences = [
    s("HEADER.verbatim", true, "Use this prompt exactly as written; do not rewrite it or add details."),
    s("GOAL.edit", true, "Edit input image 1 into a single polished casual mobile-game icon. Change only the rendering style."),
    s("STYLE.look", false, "Look: soft, rounded, toy-like shapes with clean painted surfaces."),
    s("STYLE.color", false, "Color: bright, saturated pastel palette with a few strong accent colors."),
    s("STYLE.outline", true, "No outline: keep a crisp, clean silhouette edge with no stroke, no sticker border and no glow."),
    s(`SUBJECT.keep`, true, `Keep the ${kind}'s shape, parts and color layout.`),
    s(
      "CAMERA.view",
      true,
      `Keep the capture's viewpoint exactly: camera looking down at about ${Number(camera?.pitch ?? 30)} degrees, object turned about ${Math.abs(Number(camera?.yaw ?? 28))} degrees so the front face and the ${Number(camera?.yaw ?? 28) < 0 ? "left" : "right"} side face are visible.`,
    ),
    s("AVOID.tech", true, "No background, no text, no logo, no watermark, no frame, no second scene, nothing cut off."),
    s("OUTPUT.spec", true, "Output: one centered object on a transparent background, square canvas."),
  ];
  return { packId: "default", templateVersion: "2", sentences, text: sentences.map((x) => x.text).join("\n"), addable: ["STYLE", "SUBJECT", "AVOID"] };
}
