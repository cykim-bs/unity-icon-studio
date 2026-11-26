// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { setLang } from "./i18n";
import { newRun } from "./run-state";
import { renderDetails, type DetailsHooks, type DetailsView, type GenView } from "./screens/details";
import type { PrefabItem, ProjectSummary } from "./types";

const project: ProjectSummary = {
  id: "p1",
  packId: "default",
  exists: true,
  mode: "batch",
  reason: "closed",
  lock: "none",
  path: "C:\\Projects\\Toy",
  name: "Toy",
  unityVersion: "6000.0.1f1",
  unityRevision: null,
  urp: true,
  pipelinePackage: false,
  branch: "main",
  isGit: true,
};

const prefab = (name: string): PrefabItem => ({
  guid: name,
  path: `Assets/Props/${name}.prefab`,
  name,
  folder: "Assets/Props",
  class: "3d",
  kind: "prop",
  kindSource: "default",
  skinned: false,
  nested: 0,
  mtimeMs: 0,
});

const gen = (extra: Partial<GenView> = {}): GenView => ({
  note: "",
  cli: "claude",
  busy: "",
  diff: null,
  error: null,
  errorStage: null,
  failure: null,
  candidates: 1,
  outline: "code",
  background: "auto",
  result: null,
  exportName: "",
  exportSizes: [256],
  ...extra,
});

function view(extra: Partial<DetailsView>): DetailsView {
  return {
    project,
    pack: null,
    picked: [prefab("Crate")],
    lang: "en",
    capture: { running: false, progress: "", error: null, images: {}, failures: {}, outputs: {} },
    ready: null,
    gen: gen(),
    run: null,
    now: 0,
    bg: "checker",
    exportDir: "",
    exportRule: "icon_{prefab}",
    opened: null,
    ...extra,
  };
}

function hooks(): DetailsHooks {
  const h = {} as Record<string, unknown>;
  return new Proxy(h, { get: (t, k) => (t[k as string] ??= vi.fn()) }) as unknown as DetailsHooks;
}

describe("right column", () => {
  it("one prefab: the one accent button makes the icon, even before a capture", () => {
    setLang("en");
    const box = document.createElement("div");
    const h = hooks();
    renderDetails(box, view({}), h);
    const make = box.querySelector<HTMLButtonElement>("#make-btn")!;
    expect(make.disabled).toBe(false);
    expect(box.querySelectorAll(".act-btn.primary")).toHaveLength(1);
    make.click();
    expect(h.make).toHaveBeenCalledWith([expect.objectContaining({ name: "Crate" })]);
    expect(box.querySelector<HTMLButtonElement>("#rewrite-btn")!.disabled).toBe(true);
  });

  it("while it runs: the step row and a stop button; options are locked", () => {
    setLang("en");
    const box = document.createElement("div");
    const run = newRun([{ asset: "Assets/Props/Crate.prefab", name: "Crate", captured: false }], 0);
    const h = hooks();
    renderDetails(box, view({ run }), h);
    expect(box.querySelectorAll(".stepper .step")).toHaveLength(6);
    box.querySelector<HTMLButtonElement>("#cancel-btn")!.click();
    expect(h.cancel).toHaveBeenCalled();
    expect(box.querySelector<HTMLTextAreaElement>("#make-note")!.disabled).toBe(true);
  });

  it("a failed rewrite offers the other CLI and going on without the note", () => {
    setLang("en");
    const box = document.createElement("div");
    const h = hooks();
    const failure = { kind: "bad_json", cli: "claude", message: "not JSON", exitCode: 0 };
    renderDetails(box, view({ gen: gen({ note: "blue", error: "rewrite:bad_json", errorStage: "rewrite", failure }) }), h);
    const buttons = [...box.querySelectorAll<HTMLButtonElement>(".fail-box button")];
    expect(buttons.map((b) => b.textContent)).toEqual(["Again with Codex", "Go on without the note"]);
    buttons[1].click();
    expect(h.retry).toHaveBeenCalledWith("withoutNote", expect.any(Array));
  });

  it("a usage limit has no retry button", () => {
    setLang("en");
    const box = document.createElement("div");
    const failure = { kind: "limit", cli: "codex", message: "limit", exitCode: 1 };
    renderDetails(box, view({ gen: gen({ error: "image:limit", errorStage: "generate", failure }) }), hooks());
    expect(box.querySelector(".fail-box")).not.toBeNull();
    expect(box.querySelectorAll(".fail-box button")).toHaveLength(0);
  });

  it("several prefabs: one note for all and 「Make N icons」", () => {
    setLang("en");
    const box = document.createElement("div");
    const h = hooks();
    const title = renderDetails(box, view({ picked: [prefab("A"), prefab("B")] }), h);
    expect(title).toBe("2 selected");
    const make = box.querySelector<HTMLButtonElement>("#make-btn")!;
    expect(make.textContent).toBe("Make 2 icons");
    make.click();
    expect(h.make).toHaveBeenCalledWith([expect.objectContaining({ name: "A" }), expect.objectContaining({ name: "B" })]);
  });

  it("export needs a folder: the button waits and the picker is offered", () => {
    setLang("en");
    const box = document.createElement("div");
    const post = {
      ok: true,
      jobDir: "j",
      candidate: 1,
      kind: "prop",
      master: "m",
      options: { outline: "code", background: "auto" },
      post: { alphaSource: "native", removedParts: 0, layout: { strokePx: 4 } },
      generated: { report: { pass: true, failed: [], outlineOnly: false, items: [], metrics: { outlinePx: 0, outlineColor: "" } }, iou: 0.9, modelOutline: false, nativeAlpha: true },
      sizes: [{ size: 256, path: "x", checks: { pass: true, failed: [], outlineOnly: false, items: [], metrics: { outlinePx: 3.6, outlineColor: "#753719" } } }],
      pass: true,
      elapsedMs: 1,
    };
    const result = { job: "j", name: "Crate", kind: "prop", cands: [{ cand: 1, post, failure: null, passed: 13, checked: 13 }], best: 1, cand: 1, icons: { 1: "data:," }, raws: {}, showRaw: false, exported: null, outlineKept: false };
    const h = hooks();
    renderDetails(box, view({ gen: gen({ result }) }), h);
    expect(box.querySelector<HTMLButtonElement>("#export-btn")!.disabled).toBe(true);
    box.querySelector<HTMLButtonElement>("#export-pick")!.click();
    expect(h.pickExportFolder).toHaveBeenCalled();
    const box2 = document.createElement("div");
    renderDetails(box2, view({ gen: gen({ result }), exportDir: "C:\\Icons" }), hooks());
    expect(box2.querySelector<HTMLButtonElement>("#export-btn")!.disabled).toBe(false);
    expect(box2.querySelector<HTMLInputElement>("#export-name")!.placeholder).toBe("icon_Crate");
  });
});
