/**
 * Right column. Nothing picked: what to do and the readiness list. One prefab: the capture
 * preview, its facts and kind, the note (with the rewrite before/after), the make options, the
 * one accent button 「Make icon」, the step row while it runs, and the result: candidates, the
 * finished icon on a chosen background, the checks, 「finish again」 and the export. Several: how
 * many of which kinds, one note for all, 「Make N icons」 and a progress list. A job opened from
 * the 「Runs」 tab shows its result the same way.
 */

import { capturable, captureErrorText, itemErrorText } from "../capture-text";
import { renderChecks } from "../components/checks-table";
import { renderPromptDiff } from "../components/prompt-diff";
import { renderCameraBox } from "../components/camera-box";
import { renderStepper } from "../components/stepper";
import { t } from "../i18n";
import { KIND_CHIPS } from "../prefab-filter";
import { failureText, generateErrorText, readyRow } from "../prompt-text";
import { runCounts, type ItemRunView, type RunView } from "../run-state";
import type { CameraView, ExportReport, Failure, PackSummary, PostJobReport, PrefabItem, ProjectSummary, Readiness, RewriteReport } from "../types";
import { modeText, reasonText } from "./projects";

/** Capture state of the open project, kept by the page (main.ts). */
export interface CaptureView {
  running: boolean;
  /** Progress line while running */
  progress: string;
  /** Last run's error, as a code */
  error: string | null;
  /** Picture (data: URL) of the last capture, by asset path */
  images: Record<string, string>;
  /** Item error of the last capture, by asset path */
  failures: Record<string, string>;
  /** The capture PNG on disk, by asset path (what generation starts from) */
  outputs: Record<string, string>;
  /** The composition changed after this capture, by asset path: a new capture is needed */
  stale?: Record<string, boolean>;
}

/** One candidate of a result. */
export interface CandView {
  cand: number;
  post: PostJobReport | null;
  failure: Failure | null;
  passed: number;
  checked: number;
  /** 2 and more: a try with corrections after a failed fidelity check */
  attempt?: number;
  correction?: string | null;
}

/** A finished (or failed) job: what the result panel shows. */
export interface ResultView {
  job: string;
  /** The prefab's name (export names use it) */
  name: string;
  kind: string;
  cands: CandView[];
  best: number | null;
  /** The candidate shown */
  cand: number;
  /** Icon and raw picture (data: URLs) by candidate */
  icons: Record<number, string>;
  raws: Record<number, string>;
  /** Fidelity maps (data: URLs) by candidate */
  maps?: Record<number, string>;
  showRaw: boolean;
  exported: ExportReport | null;
  outlineKept: boolean;
  /** Opened from the 「Runs」 tab: when, which project, with what note */
  from?: { at: string; project: string; note: string; asset: string } | null;
}

/** The note, options and result of the picked prefab (main.ts keeps it). */
export interface GenView {
  note: string;
  /** claude or codex */
  cli: string;
  busy: "" | "rewrite" | "post" | "export";
  diff: RewriteReport | null;
  /** Why the last make or rewrite did not go through (a code), and at which step */
  error: string | null;
  errorStage: string | null;
  failure: Failure | null;
  candidates: number;
  outline: string;
  background: string;
  /** strict (keep the capture, add nothing) or stylized */
  fidelity: string;
  result: ResultView | null;
  /** File name typed for the export ("" = the settings' rule) */
  exportName: string;
  exportSizes: number[];
}

export interface DetailsView {
  project: ProjectSummary | null;
  pack: PackSummary | null;
  picked: PrefabItem[];
  lang: string;
  capture: CaptureView;
  ready: Readiness | null;
  gen: GenView;
  run: RunView | null;
  now: number;
  /** Preview background: checker, white, black */
  bg: string;
  exportDir: string;
  exportRule: string;
  /** A job opened from the 「Runs」 tab (shown instead of the picked prefab) */
  opened: ResultView | null;
  /** The camera of the one picked prefab (null while it loads) */
  camera?: CameraView | null;
}

export type RetryKind = "otherCli" | "withoutNote" | "again" | "noRefs";

export interface DetailsHooks {
  setKind(item: PrefabItem, kind: string | null): void;
  openFolder(folder: string): void;
  copy(text: string): void;
  capture(items: PrefabItem[]): void;
  note(text: string): void;
  rewriter(cli: string): void;
  rewrite(item: PrefabItem): void;
  showPrompt(item: PrefabItem): void;
  make(items: PrefabItem[]): void;
  retry(kind: RetryKind, items: PrefabItem[]): void;
  cancel(): void;
  option(key: "candidates" | "outline" | "background" | "fidelity", value: string): void;
  /** After a failed strict picture: make it again stylized, or go to the note */
  fidelityChoice(kind: "stylized" | "note"): void;
  pickCand(n: number): void;
  toggleRaw(): void;
  setBg(bg: string): void;
  repost(): void;
  exportIcon(): void;
  exportPassed(): void;
  pickExportFolder(): void;
  exportName(text: string): void;
  exportSize(size: number, on: boolean): void;
  openPath(path: string): void;
  checkReady(): void;
  closeOpened(): void;
  rerunOpened(): void;
  /** A person's composition values (`null` clears a key), all cleared, or a note's camera wish */
  setCamera(item: PrefabItem, patch: Record<string, unknown>): void;
  resetCamera(item: PrefabItem): void;
  cameraHint(item: PrefabItem, camera: string): void;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className = "", text = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
}

function button(label: string, onClick: () => void, className = "act-btn", disabled = false): HTMLButtonElement {
  const b = el("button", className, label);
  b.type = "button";
  b.disabled = disabled;
  b.addEventListener("click", onClick);
  return b;
}

function packName(pack: PackSummary | null, lang: string): string {
  if (!pack) return "-";
  const n = pack.name;
  if (typeof n === "string") return n;
  if (n && typeof n === "object") return n[lang] ?? n.en ?? pack.id;
  return pack.id;
}

function facts(rows: [string, string | Node][]): HTMLElement {
  const dl = el("dl", "details");
  for (const [label, value] of rows) {
    dl.append(el("dt", "", label));
    const dd = el("dd");
    if (typeof value === "string") dd.textContent = value;
    else dd.append(value);
    dl.append(dd);
  }
  return dl;
}

function select(label: string, options: [string, string][], value: string, disabled: boolean, onChange: (v: string) => void): HTMLSelectElement {
  const s = el("select", "make-cli");
  s.setAttribute("aria-label", label);
  for (const [v, text] of options) s.append(new Option(text, v));
  s.value = value;
  s.disabled = disabled;
  s.addEventListener("change", () => onChange(s.value));
  return s;
}

function readiness(ready: Readiness | null, hooks: DetailsHooks): HTMLElement {
  const list = el("div", "ready-list");
  list.append(el("h3", "", t("ready.title")));
  for (const [key, name] of [["ready.codex", "codex"], ["ready.claude", "claude"], ["ready.unityCli", "unityCli"]] as const) {
    const row = el("div", "ready-row");
    row.dataset.ready = name;
    const r = readyRow(name, ready);
    row.append(el("span", `ready-dot ${r.level}`), el("span", "ready-name", t(key)), el("span", r.level === "bad" ? "warn-text" : "muted", r.text));
    list.append(row);
  }
  list.append(button(t("ready.check"), () => hooks.checkReady(), "link-btn"));
  return list;
}

// ---------------------------------------------------------------- picture boxes

const BGS = ["checker", "white", "black"] as const;

function bgChips(view: DetailsView, hooks: DetailsHooks): HTMLElement {
  const box = el("div", "bg-chips");
  box.setAttribute("role", "group");
  box.setAttribute("aria-label", t("preview.bg"));
  for (const b of BGS) {
    const chip = button(t(`preview.bg.${b}`), () => hooks.setBg(b), "chip-btn small");
    chip.setAttribute("aria-pressed", String(view.bg === b));
    chip.title = t("preview.bgKey");
    box.append(chip);
  }
  return box;
}

function previewBox(item: PrefabItem, cap: CaptureView, bg: string): HTMLElement {
  const box = el("figure", `capture-preview bg-${bg}${cap.stale?.[item.path] ? " stale" : ""}`);
  if (cap.stale?.[item.path]) box.title = t("camera.stale");
  box.id = "capture-preview";
  box.tabIndex = 0;
  const src = cap.images[item.path];
  if (src) {
    const img = el("img");
    img.src = src;
    img.alt = t("capture.preview");
    img.draggable = false;
    box.append(img);
  } else {
    const fail = cap.failures[item.path];
    box.append(el("figcaption", fail ? "warn-text" : "muted", fail ? t("capture.itemFailed", { text: itemErrorText(fail) }) : t("capture.none")));
  }
  return box;
}

// ---------------------------------------------------------------- make: note, options, button, steps

/** Buttons for a failed make: another rewriter, without the note, again, without references. */
function failureActions(g: GenView, items: PrefabItem[], hooks: DetailsHooks): HTMLElement | null {
  if (!g.error) return null;
  const box = el("div", "fail-box");
  box.setAttribute("role", "alert");
  const text = g.failure ? failureText(g.failure) : g.error.startsWith("rewrite:") || g.error === "busy" ? generateErrorText(g.error) : runErrorText(g.error);
  box.append(el("p", "warn-text", text));
  const actions = el("div", "body-actions");
  if (g.errorStage === "rewrite") {
    const other = g.cli === "claude" ? "Codex" : "Claude";
    actions.append(button(t("fail.otherCli", { cli: other }), () => hooks.retry("otherCli", items)), button(t("fail.withoutNote"), () => hooks.retry("withoutNote", items)));
  } else if (g.errorStage === "generate") {
    const kind = g.failure?.kind ?? "";
    if (kind !== "limit" && kind !== "auth" && kind !== "not_found") {
      actions.append(button(t("fail.again"), () => hooks.retry("again", items)), button(t("fail.noRefs"), () => hooks.retry("noRefs", items)));
    }
  } else if (g.errorStage === "capture" || g.errorStage === "post") {
    actions.append(button(t("fail.again"), () => hooks.retry("again", items)));
  }
  if (actions.childElementCount) box.append(actions);
  return box;
}

/** A run error code in words (`image:<kind>`, a capture code, …). */
export function runErrorText(code: string): string {
  if (code.startsWith("image:")) return failureText({ kind: code.slice(6), cli: "codex", message: "", exitCode: null });
  if (code === "noImage") return t("llm.err.no_image", { cli: "Codex", text: "" });
  if (/^(blocked|hook|editor|entry|batch|unityCli|noItems|attach)/.test(code)) return captureErrorText(code);
  if (/^[a-z_]+$/.test(code)) return t("capture.itemFailed", { text: itemErrorText(code) });
  return generateErrorText(code);
}

function optionsRow(g: GenView, disabled: boolean, hooks: DetailsHooks): HTMLElement {
  const row = el("div", "make-row make-options");
  const fid = select(t("make.fidelity"), ["strict", "stylized"].map((v) => [v, t(`make.fidelity.${v}`)]), g.fidelity, disabled, (v) => hooks.option("fidelity", v));
  fid.id = "make-fidelity";
  fid.title = t(`make.fidelity.${g.fidelity}.hint`);
  row.append(el("span", "muted", t("make.fidelity")), fid);
  const cands = select(t("make.candidates"), ["1", "2", "3", "4"].map((n) => [n, n]), String(g.candidates), disabled, (v) => hooks.option("candidates", v));
  const outline = select(t("result.outline"), ["code", "model", "none"].map((v) => [v, t(`result.outline.${v}`)]), g.outline, disabled, (v) => hooks.option("outline", v));
  const bg = select(t("make.background"), ["auto", "key"].map((v) => [v, t(`make.background.${v}`)]), g.background, disabled, (v) => hooks.option("background", v));
  row.append(el("span", "muted", t("make.candidates")), cands, el("span", "muted", t("result.outline")), outline, el("span", "muted", t("make.background")), bg);
  return row;
}

function noteBox(g: GenView, disabled: boolean, hooks: DetailsHooks, onChange: (empty: boolean) => void): HTMLTextAreaElement {
  const note = el("textarea", "make-note");
  note.id = "make-note";
  note.rows = 3;
  note.value = g.note;
  note.placeholder = t("make.notePlaceholder");
  note.setAttribute("aria-label", t("make.note"));
  note.disabled = disabled;
  // Typing keeps the focus: only the state and the buttons that depend on it follow (no redraw)
  note.addEventListener("input", () => {
    hooks.note(note.value);
    onChange(!note.value.trim());
  });
  return note;
}

function makeButton(label: string, disabled: boolean, onClick: () => void): HTMLButtonElement {
  const make = button(label, onClick, "act-btn primary make-btn", disabled);
  make.id = "make-btn";
  make.title = t("make.makeHint");
  return make;
}

function makeSection(item: PrefabItem, view: DetailsView, hooks: DetailsHooks): HTMLElement {
  const g = view.gen;
  const run = view.run;
  const mine = run?.items.find((i) => i.asset === item.path) ?? null;
  const running = !!run?.running;
  const busy = running || !!g.busy;
  const box = el("section", "make-box");
  box.append(el("h3", "make-title", t("make.title")));
  const preview = button(g.busy === "rewrite" ? t("make.rewriting") : t("make.preview"), () => hooks.rewrite(item), "act-btn", busy || !g.note.trim());
  preview.id = "rewrite-btn";
  preview.title = t("make.previewHint");
  const hint = el("p", "body-hint", t("make.emptyNote"));
  hint.hidden = !!g.note.trim();
  const note = noteBox(g, busy, hooks, (empty) => {
    preview.disabled = busy || empty;
    hint.hidden = !empty;
  });
  const cliRow = el("div", "make-row");
  const rewriter = select(t("make.rewriter"), [["claude", "Claude"], ["codex", "Codex"]], g.cli, busy, (v) => hooks.rewriter(v));
  rewriter.id = "rewriter-select";
  cliRow.append(
    el("span", "muted", t("make.rewriter")),
    rewriter,
    preview,
    Object.assign(button(t("make.showPrompt"), () => hooks.showPrompt(item), "link-btn"), { id: "show-prompt-btn" }),
  );
  box.append(note, hint, cliRow);
  // A failed rewrite says why in the failure box below (with its buttons)
  if (g.diff && !(g.diff.failure && g.errorStage === "rewrite")) box.append(renderPromptDiff(g.diff, busy ? undefined : (c) => hooks.cameraHint(item, c)));
  box.append(optionsRow(g, busy, hooks));
  const actions = el("div", "body-actions make-actions");
  const blocked = view.project?.mode === "blocked" && !view.capture.outputs[item.path];
  const make = makeButton(running && mine ? t("make.making") : t("make.make"), busy || blocked, () => hooks.make([item]));
  if (blocked) make.title = t("make.blocked", { why: reasonText(view.project!) });
  actions.append(make);
  if (running && mine) {
    const stop = button(run!.cancelling ? t("make.stopping") : t("make.cancel"), () => hooks.cancel(), "act-btn", run!.cancelling);
    stop.id = "cancel-btn";
    stop.title = "Esc";
    actions.append(stop);
  }
  box.append(actions);
  if (mine) {
    box.append(renderStepper(mine, view.now));
    if (running && mine.steps.capture.state === "run") {
      box.append(el("p", "body-hint", view.project?.mode === "attach" ? t("make.captureAttach") : t("make.captureBatch", { s: Math.round((view.now - (mine.steps.capture.startMs ?? view.now)) / 1000) })));
    }
    if (running && mine.steps.generate.state === "run") box.append(el("p", "body-hint", t("make.generatingHint")));
    if (run!.cancelling) box.append(el("p", "body-hint", t("make.stopHint")));
  }
  const fail = !running ? failureActions(g, [item], hooks) : null;
  if (fail) box.append(fail);
  if (g.result && !running) box.append(resultSection(g.result, view, hooks, false));
  return box;
}

// ---------------------------------------------------------------- result

function candTabs(r: ResultView, hooks: DetailsHooks): HTMLElement {
  const row = el("div", "cand-tabs");
  row.setAttribute("role", "tablist");
  row.setAttribute("aria-label", t("result.cands"));
  for (const c of r.cands) {
    const label = t("result.cand", { n: c.cand });
    const b = button(label, () => hooks.pickCand(c.cand), "cand-tab");
    b.setAttribute("role", "tab");
    b.setAttribute("aria-selected", String(c.cand === r.cand));
    b.dataset.cand = String(c.cand);
    if (c.post) b.append(el("span", c.post.pass ? "cand-score ok" : "cand-score bad", `${c.passed}/${c.checked}`));
    else b.append(el("span", "cand-score bad", "!"));
    if (c.cand === r.best) b.append(el("span", "cand-best", t("result.best")));
    row.append(b);
  }
  return row;
}

function resultSection(r: ResultView, view: DetailsView, hooks: DetailsHooks, opened: boolean): HTMLElement {
  const g = view.gen;
  const box = el("section", "result-box");
  box.id = "result";
  box.append(el("h3", "make-title", t("result.title")));
  if (r.cands.length > 1) box.append(candTabs(r, hooks));
  const c = r.cands.find((x) => x.cand === r.cand) ?? r.cands[0];
  const p = c?.post ?? null;
  const head = el("div", "make-row");
  head.append(bgChips(view, hooks));
  if (r.raws[r.cand] || r.icons[r.cand]) {
    const raw = button(t("result.showRaw"), () => hooks.toggleRaw(), "chip-btn small");
    raw.setAttribute("aria-pressed", String(r.showRaw));
    head.append(raw);
  }
  box.append(head);
  const src = r.showRaw ? r.raws[r.cand] : r.icons[r.cand];
  if (src) {
    const fig = el("figure", `capture-preview icon-preview bg-${view.bg}`);
    fig.tabIndex = 0;
    const sizes = r.showRaw ? [256] : [256, 128, 64];
    for (const size of sizes) {
      const img = el("img", `icon-${size}`);
      img.src = src;
      img.width = size;
      img.height = size;
      img.alt = r.showRaw ? t("make.raw") : t("result.icon", { size });
      img.draggable = false;
      fig.append(img);
    }
    if (p && !r.showRaw) {
      const s = p.sizes[0];
      const iou = p.generated.iou === null ? "-" : p.generated.iou.toFixed(2);
      fig.append(el("figcaption", "muted", t("result.info", { size: s.size, stroke: s.checks.metrics.outlinePx.toFixed(1), alpha: t(`result.alpha.${p.post.alphaSource}`), iou })));
    }
    box.append(fig);
  } else if (c?.failure) {
    box.append(el("p", "warn-text", failureText(c.failure)));
  }
  if (r.outlineKept) box.append(el("p", "body-hint", t("result.outlineKept")));
  if (p) box.append(renderChecks(p.sizes[0].checks, p.generated.report, p.fidelity?.report ?? null));
  if (p && p.fidelityMode === "strict") box.append(fidelityBlock(c, r, hooks));
  // finish again (no new picture)
  const opts = el("div", "make-row");
  opts.append(
    el("span", "muted", t("result.outline")),
    select(t("result.outline"), ["code", "none", "model"].map((v) => [v, t(`result.outline.${v}`)]), g.outline, !!g.busy, (v) => hooks.option("outline", v)),
    el("span", "muted", t("result.background")),
    select(t("result.background"), ["auto", "key"].map((v) => [v, t(`result.background.${v}`)]), g.background, !!g.busy, (v) => hooks.option("background", v)),
  );
  const again = button(g.busy === "post" ? t("result.posting") : t("result.repost"), () => hooks.repost(), "act-btn", !!g.busy || !c?.post);
  again.id = "repost-btn";
  again.title = t("result.repostHint");
  opts.append(again);
  box.append(opts);
  box.append(exportBlock(r, view, hooks, !!p));
  if (opened) {
    const row = el("div", "body-actions");
    row.append(button(t("runs.rerun"), () => hooks.rerunOpened(), "act-btn"), button(t("runs.close"), () => hooks.closeOpened(), "link-btn"));
    box.append(row);
  }
  return box;
}

/** Strict pictures: the reviews' findings, the fidelity map, and the choices when it failed. */
function fidelityBlock(c: CandView, r: ResultView, hooks: DetailsHooks): HTMLElement {
  const p = c.post!;
  const box = el("section", "fidelity-box");
  box.setAttribute("aria-label", t("fidelity.title"));
  if ((c.attempt ?? 1) > 1) box.append(el("p", "body-hint", t("fidelity.retried", { n: (c.attempt ?? 2) - 1 })));
  for (const rv of p.reviews ?? []) {
    const name = rv.cli === "claude" ? "Claude" : "Codex";
    const line = el("p", `review-head ${rv.ok && rv.pass ? "ok" : "bad"}`);
    line.textContent = !rv.ok
      ? t("review.error", { cli: name, code: rv.failure ? `${rv.failure.kind}` : "?" })
      : rv.pass
        ? t("review.pass", { cli: name })
        : t("review.fail", { cli: name, n: rv.failing.length });
    box.append(line);
    const list = el("ul", "review-list");
    for (const f of rv.failing) list.append(el("li", "warn-text", `${t(`review.kind.${f.kind}`)}: ${f.what}${f.detail ? ` (${f.detail})` : ""}`));
    for (const f of rv.notes) list.append(el("li", "muted", `${t(`review.kind.${f.kind}`)}: ${f.what}${f.detail ? ` (${f.detail})` : ""}`));
    if (list.childElementCount) box.append(list);
  }
  if (p.fidelity?.suspect && !(p.reviews ?? []).some((x) => x.ok)) box.append(el("p", "body-hint warn-text", t("fidelity.suspectNoReview")));
  const map = r.maps?.[c.cand];
  if (map) {
    const d = el("details", "fidelity-map");
    d.append(el("summary", "", t("fidelity.map")));
    const img = el("img");
    img.src = map;
    img.alt = t("fidelity.map");
    img.draggable = false;
    d.append(img, el("p", "muted", t("fidelity.mapLegend")));
    box.append(d);
  }
  const reviewFailed = (p.reviews ?? []).some((x) => x.ok && !x.pass);
  const unreviewed = !!p.fidelity?.suspect && !(p.reviews ?? []).some((x) => x.ok);
  if (!p.pass && (p.fidelity?.report.pass === false || reviewFailed || unreviewed)) {
    const row = el("div", "body-actions");
    row.append(
      button(t("fidelity.toStylized"), () => hooks.fidelityChoice("stylized"), "act-btn"),
      button(t("fidelity.fixNote"), () => hooks.fidelityChoice("note"), "act-btn"),
    );
    box.append(el("p", "body-hint", t("fidelity.failedHint")), row);
  }
  return box;
}

/** File name, sizes, folder (with a picker) and 「Export」. */
function exportBlock(r: ResultView, view: DetailsView, hooks: DetailsHooks, ready: boolean): HTMLElement {
  const g = view.gen;
  const box = el("div", "export-box");
  const nameRow = el("div", "make-row");
  const name = el("input", "export-name");
  name.id = "export-name";
  name.type = "text";
  name.spellcheck = false;
  name.value = g.exportName;
  name.placeholder = view.exportRule.replace("{prefab}", r.name);
  name.setAttribute("aria-label", t("export.name"));
  name.addEventListener("input", () => hooks.exportName(name.value));
  nameRow.append(el("span", "muted", t("export.name")), name, el("span", "muted", ".png"));
  const sizes = el("div", "make-row export-sizes");
  sizes.append(el("span", "muted", t("export.sizes")));
  for (const size of [256, 512, 1024]) {
    const label = el("label", "check-label");
    const box2 = el("input");
    box2.type = "checkbox";
    box2.checked = g.exportSizes.includes(size);
    box2.dataset.size = String(size);
    box2.addEventListener("change", () => hooks.exportSize(size, box2.checked));
    label.append(box2, document.createTextNode(String(size)));
    sizes.append(label);
  }
  const folder = el("div", "make-row export-folder");
  const where = el("span", view.exportDir ? "export-dir" : "export-dir muted", view.exportDir || t("export.noFolder"));
  where.title = view.exportDir;
  const pick = button(t("export.pick"), () => hooks.pickExportFolder(), "link-btn");
  pick.id = "export-pick";
  folder.append(el("span", "muted", t("export.folder")), where, pick);
  const actions = el("div", "body-actions");
  const out = button(g.busy === "export" ? t("result.exporting") : t("result.export"), () => hooks.exportIcon(), "act-btn", !!g.busy || !ready || !view.exportDir || !g.exportSizes.length);
  out.id = "export-btn";
  out.title = t("result.exportHint");
  actions.append(out);
  box.append(nameRow, sizes, folder, actions);
  const x = r.exported;
  if (x?.ok) {
    const line = el("p", "body-hint export-done");
    line.append(document.createTextNode(t("result.exported", { n: x.files.length, folder: x.folder }) + " "));
    line.append(button(t("export.openFolder"), () => hooks.openPath(x.folder), "link-btn"));
    box.append(line);
  } else if (x?.error) {
    box.append(el("p", "body-hint warn-text", exportErrorText(x.error)));
  }
  return box;
}

/** Why an export did not go through. */
export function exportErrorText(code: string): string {
  if (code.startsWith("insideProject:")) return t("result.err.insideProject", { path: code.slice("insideProject:".length) });
  if (code === "noExportDir") return t("result.err.noExportDir");
  if (code.startsWith("noIcon:")) return t("result.err.noIcon", { size: code.slice("noIcon:".length) });
  return t("result.err.other", { code });
}

// ---------------------------------------------------------------- one, many, opened

function kindSourceText(item: PrefabItem): string {
  switch (item.kindSource) {
    case "rule":
      return t("details.kindByRule", { rule: item.rule ?? "" });
    case "skinned":
      return t("details.kindBySkin");
    case "override":
      return t("details.kindByPerson");
    default:
      return t("details.kindDefault");
  }
}

function captureButton(items: PrefabItem[], view: DetailsView, hooks: DetailsHooks, label: string): HTMLButtonElement {
  const cap = view.capture;
  const b = button(cap.running ? t("capture.running") : label, () => hooks.capture(items), "act-btn", cap.running || !!view.run?.running || items.length === 0);
  b.id = "capture-btn";
  b.title = t("capture.takeHint");
  return b;
}

function captureNotes(cap: CaptureView): HTMLElement[] {
  const out: HTMLElement[] = [];
  if (cap.running && cap.progress) out.push(el("p", "body-hint", cap.progress));
  else if (cap.error) out.push(el("p", "body-hint warn-text", captureErrorText(cap.error)));
  return out;
}

function one(item: PrefabItem, hooks: DetailsHooks, view: DetailsView): HTMLElement[] {
  const cap = view.capture;
  const head = el("div", "details-head");
  head.append(el("h2", "details-title", item.name));
  const path = button(item.path, () => hooks.copy(item.path), "link-btn path");
  path.title = t("details.copyPath");

  const kind = el("select", "kind-select");
  kind.setAttribute("aria-label", t("details.kind"));
  kind.append(new Option(t("details.kindAuto"), ""), ...KIND_CHIPS.map((k) => new Option(t(`kind.${k}`), k)));
  kind.value = item.kindSource === "override" ? item.kind : "";
  kind.addEventListener("change", () => hooks.setKind(item, kind.value || null));
  const kindBox = el("div", "kind-box");
  kindBox.append(el("span", "kind-now", t(`kind.${item.kind}`)), kind);

  const rows: [string, string | Node][] = [
    [t("details.path"), path],
    [t("details.class"), t(`class.${item.class}`)],
    [t("details.kind"), kindBox],
    [t("details.kindWhy"), kindSourceText(item)],
  ];
  const actions = el("div", "body-actions");
  const ok = capturable(item.class);
  if (ok) actions.append(captureButton([item], view, hooks, cap.images[item.path] ? t("capture.retake") : t("capture.take")));
  actions.append(button(t("details.openFolder"), () => hooks.openFolder(item.folder)));
  const out: HTMLElement[] = [head];
  if (view.project?.mode === "blocked") out.push(el("p", "body-hint warn-text blocked-note", t("make.blocked", { why: reasonText(view.project) })));
  if (ok) {
    const top = el("div", "make-row");
    top.append(bgChips(view, hooks));
    out.push(top, previewBox(item, cap, view.bg));
  }
  out.push(facts(rows), actions, ...captureNotes(cap));
  if (ok) {
    const busy = cap.running || !!view.run?.running;
    out.push(
      renderCameraBox(view.camera ?? null, !!cap.stale?.[item.path], busy, {
        set: (patch) => hooks.setCamera(item, patch),
        reset: () => hooks.resetCamera(item),
        retake: () => hooks.capture([item]),
      }),
    );
    out.push(makeSection(item, view, hooks));
  }
  else out.push(el("p", "body-hint warn-text", t("details.not3d")));
  return out;
}

function progressRow(it: ItemRunView): HTMLElement {
  const li = el("li", `run-row run-${it.state}`);
  li.dataset.asset = it.asset;
  const step = (["check", "post", "generate", "rewrite", "prompt", "capture"] as const).find((s) => it.steps[s].state !== "wait" && it.steps[s].state !== "skip");
  const now = step ? t(`steps.${step}`) : "";
  li.append(el("span", "run-name", it.name), el("span", "run-state", t(`run.state.${it.state}`)), el("span", "muted", it.state === "running" ? now : ""));
  if (it.result?.state === "ok") li.append(el("span", it.result.pass ? "cand-score ok" : "cand-score bad", it.result.pass ? t("run.pass") : t("run.notPass")));
  else if (it.result?.error) li.append(el("span", "warn-text", runErrorText(it.result.error)));
  return li;
}

function many(items: PrefabItem[], hooks: DetailsHooks, view: DetailsView): HTMLElement[] {
  const g = view.gen;
  const run = view.run;
  const running = !!run?.running;
  const busy = running || !!g.busy;
  const counts = new Map<string, number>();
  for (const i of items) counts.set(i.kind, (counts.get(i.kind) ?? 0) + 1);
  const head = el("div", "details-head");
  head.append(el("h2", "details-title", t("details.many", { n: items.length })));
  const rows: [string, string][] = KIND_CHIPS.filter((k) => counts.has(k)).map((k) => [t(`kind.${k}`), String(counts.get(k))]);
  const targets = items.filter((i) => capturable(i.class));
  const actions = el("div", "body-actions");
  actions.append(captureButton(targets, view, hooks, t("capture.takeMany", { n: targets.length })));
  const strip = el("div", "capture-strip");
  for (const i of targets) {
    const src = view.capture.images[i.path];
    if (!src) continue;
    const img = el("img");
    img.src = src;
    img.alt = i.name;
    img.title = i.name;
    img.draggable = false;
    strip.append(img);
  }
  const out: HTMLElement[] = [head, facts(rows), actions, ...captureNotes(view.capture)];
  if (strip.childElementCount) out.push(strip);
  const box = el("section", "make-box");
  box.append(el("h3", "make-title", t("make.titleMany")));
  const hint = el("p", "body-hint", t("make.emptyNote"));
  hint.hidden = !!g.note.trim();
  box.append(noteBox(g, busy, hooks, (empty) => (hint.hidden = !empty)), hint, optionsRow(g, busy, hooks));
  const row = el("div", "body-actions make-actions");
  row.append(makeButton(running ? t("make.making") : t("make.makeMany", { n: targets.length }), busy || !targets.length, () => hooks.make(targets)));
  if (running) row.append(button(run!.cancelling ? t("make.stopping") : t("make.cancel"), () => hooks.cancel(), "act-btn", run!.cancelling));
  box.append(row, el("p", "body-hint", t("make.manyHint")));
  if (run && run.items.length > 1) {
    const c = runCounts(run);
    box.append(el("p", "run-summary", t("run.summary", { ok: c.ok, pass: c.pass, failed: c.failed, left: c.left })));
    if (run.report?.stopped) box.append(el("p", "warn-text", runErrorText(`image:${run.report.stopped}`)));
    const list = el("ol", "run-list");
    list.id = "run-list";
    for (const it of run.items) list.append(progressRow(it));
    box.append(list);
    if (!running && c.pass) {
      const all = button(g.busy === "export" ? t("result.exporting") : t("run.exportPassed", { n: c.pass }), () => hooks.exportPassed(), "act-btn", !!g.busy || !view.exportDir);
      all.id = "export-passed-btn";
      const line = el("div", "body-actions");
      line.append(all);
      if (!view.exportDir) line.append(button(t("export.pick"), () => hooks.pickExportFolder(), "link-btn"));
      box.append(line);
      if (view.gen.result?.exported?.ok) box.append(el("p", "body-hint", t("result.exported", { n: view.gen.result.exported.files.length, folder: view.gen.result.exported.folder })));
    }
  }
  const fail = !running && view.run?.error ? failureActions({ ...g, error: view.run.error, errorStage: null }, targets, hooks) : null;
  if (fail) box.append(fail);
  out.push(box);
  return out;
}

function openedView(r: ResultView, view: DetailsView, hooks: DetailsHooks): HTMLElement[] {
  const head = el("div", "details-head");
  head.append(el("h2", "details-title", r.name));
  const out: HTMLElement[] = [head];
  if (r.from) {
    const rows: [string, string][] = [
      [t("runs.when"), atText(r.from.at)],
      [t("runs.project"), r.from.project || "-"],
      [t("details.kind"), t(`kind.${r.kind}`)],
      [t("make.note"), r.from.note.trim() ? r.from.note : t("runs.noNote")],
    ];
    out.push(facts(rows));
  }
  out.push(resultSection(r, view, hooks, true));
  return out;
}

/** `YYYYMMDD-HHMMSS` → `MM-DD HH:MM`. */
export function atText(at: string): string {
  const m = /^(\d{4})(\d{2})(\d{2})-(\d{2})(\d{2})/.exec(at);
  return m ? `${m[2]}-${m[3]} ${m[4]}:${m[5]}` : at;
}

export function renderDetails(content: HTMLElement, view: DetailsView, hooks: DetailsHooks): string {
  if (view.opened) {
    content.replaceChildren(...openedView(view.opened, view, hooks));
    return view.opened.name;
  }
  if (!view.project) {
    content.replaceChildren(readiness(view.ready, hooks));
    return "";
  }
  if (view.picked.length === 1) {
    content.replaceChildren(...one(view.picked[0], hooks, view));
    return view.picked[0].name;
  }
  if (view.picked.length > 1) {
    content.replaceChildren(...many(view.picked, hooks, view));
    return t("details.many", { n: view.picked.length });
  }
  const box = el("div", "body-empty");
  box.append(el("p", "", t("empty.pick")));
  const p = view.project;
  const summary = facts([
    [t("details.capture"), `${modeText(p)} — ${reasonText(p)}`],
    [t("details.unity"), p.unityVersion ?? "-"],
    [t("details.branch"), p.branch ?? "-"],
    [t("details.pack"), packName(view.pack, view.lang)],
    [t("details.packRules"), String(view.pack?.kindRules ?? 0)],
  ]);
  content.replaceChildren(box, summary, readiness(view.ready, hooks));
  return "";
}
