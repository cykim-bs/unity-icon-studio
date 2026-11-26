/**
 * Right column. Nothing picked: what to do and the readiness list. One prefab: its facts and the
 * kind (a person can change it; kept per project). Several: how many and of which kinds.
 */

import { capturable, captureErrorText, itemErrorText } from "../capture-text";
import { renderChecks } from "../components/checks-table";
import { renderPromptDiff } from "../components/prompt-diff";
import { t } from "../i18n";
import { KIND_CHIPS } from "../prefab-filter";
import { generateErrorText, readyRow } from "../prompt-text";
import type { ExportReport, PackSummary, PostJobReport, PrefabItem, ProjectSummary, Readiness, RewriteReport } from "../types";
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
}

/** The note, the rewrite preview and the raw picture of the picked prefab (main.ts keeps it). */
export interface GenView {
  note: string;
  /** claude or codex */
  cli: string;
  busy: "" | "rewrite" | "generate" | "post" | "export";
  diff: RewriteReport | null;
  error: string | null;
  /** The raw generated picture (data: URL) and a line about it */
  raw: string | null;
  rawInfo: string;
  /** The job folder of the last picture */
  job: string | null;
  /** Post-processing and checks of candidate 1, and the finished icon (data: URL) */
  post: PostJobReport | null;
  icon: string | null;
  /** Options the person picked for 「post-process again」 */
  outline: string;
  background: string;
  exported: ExportReport | null;
}

export interface DetailsView {
  project: ProjectSummary | null;
  pack: PackSummary | null;
  picked: PrefabItem[];
  lang: string;
  capture: CaptureView;
  ready: Readiness | null;
  gen: GenView;
}

export interface DetailsHooks {
  setKind(item: PrefabItem, kind: string | null): void;
  openFolder(folder: string): void;
  copy(text: string): void;
  capture(items: PrefabItem[]): void;
  note(text: string): void;
  rewriter(cli: string): void;
  rewrite(item: PrefabItem): void;
  generate(item: PrefabItem): void;
  checkReady(): void;
  postOption(key: "outline" | "background", value: string): void;
  repost(): void;
  exportIcon(item: PrefabItem): void;
  openPath(path: string): void;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className = "", text = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
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

function readiness(ready: Readiness | null, hooks: DetailsHooks): HTMLElement {
  const list = el("div", "ready-list");
  list.append(el("h3", "", t("ready.title")));
  for (const [key, name] of [["ready.codex", "codex"], ["ready.claude", "claude"], ["ready.unityCli", "unityCli"]] as const) {
    const row = el("div", "ready-row");
    const r = readyRow(name, ready);
    const dot = el("span", `ready-dot ${r.level}`);
    row.append(dot, el("span", "ready-name", t(key)), el("span", r.level === "bad" ? "warn-text" : "muted", r.text));
    list.append(row);
  }
  const again = el("button", "link-btn", t("ready.check"));
  again.type = "button";
  again.addEventListener("click", () => hooks.checkReady());
  list.append(again);
  return list;
}

/** Note, rewriter, 「preview the rewrite」, 「make a raw picture」, the before/after and the picture. */
function makeSection(item: PrefabItem, view: DetailsView, hooks: DetailsHooks): HTMLElement {
  const g = view.gen;
  const box = el("section", "make-box");
  box.append(el("h3", "make-title", t("make.title")));
  const note = el("textarea", "make-note");
  note.rows = 3;
  note.value = g.note;
  note.placeholder = t("make.notePlaceholder");
  note.setAttribute("aria-label", t("make.note"));
  note.disabled = !!g.busy;
  const cli = el("select", "make-cli");
  cli.setAttribute("aria-label", t("make.rewriter"));
  cli.append(new Option("Claude", "claude"), new Option("Codex", "codex"));
  cli.value = g.cli;
  cli.disabled = !!g.busy;
  cli.addEventListener("change", () => hooks.rewriter(cli.value));
  const row = el("div", "make-row");
  row.append(el("span", "muted", t("make.rewriter")), cli);
  const actions = el("div", "body-actions");
  const preview = el("button", "act-btn", g.busy === "rewrite" ? t("make.rewriting") : t("make.preview"));
  preview.type = "button";
  preview.disabled = !!g.busy || !g.note.trim();
  preview.addEventListener("click", () => hooks.rewrite(item));
  // Typing keeps the focus: only the state and this button follow (no redraw)
  note.addEventListener("input", () => {
    hooks.note(note.value);
    preview.disabled = !!g.busy || !note.value.trim();
    hint.hidden = !!note.value.trim();
  });
  const make = el("button", "act-btn", g.busy === "generate" ? t("make.generating") : t("make.generate"));
  make.type = "button";
  make.disabled = !!g.busy || !view.capture.outputs[item.path];
  make.title = view.capture.outputs[item.path] ? t("make.generateHint") : t("make.needCapture");
  make.addEventListener("click", () => hooks.generate(item));
  actions.append(preview, make);
  const hint = el("p", "body-hint", t("make.emptyNote"));
  hint.hidden = !!g.note.trim();
  box.append(note, row, actions, hint);
  if (g.error) box.append(el("p", "body-hint warn-text", generateErrorText(g.error)));
  if (g.diff) box.append(renderPromptDiff(g.diff));
  if (g.raw) {
    const fig = el("figure", "capture-preview raw-preview");
    const img = el("img");
    img.src = g.raw;
    img.alt = t("make.raw");
    img.draggable = false;
    fig.append(img, el("figcaption", "muted", g.rawInfo));
    box.append(fig);
  }
  if (g.job) box.append(resultSection(item, g, hooks));
  return box;
}

/** The finished icon, its checks, 「post-process again」 with other options, and export. */
function resultSection(item: PrefabItem, g: GenView, hooks: DetailsHooks): HTMLElement {
  const box = el("section", "result-box");
  box.append(el("h3", "make-title", t("result.title")));
  const p = g.post;
  if (g.icon) {
    const fig = el("figure", "capture-preview icon-preview");
    for (const size of [256, 64]) {
      const img = el("img", `icon-${size}`);
      img.src = g.icon;
      img.width = size;
      img.height = size;
      img.alt = t("result.icon", { size });
      img.draggable = false;
      fig.append(img);
    }
    if (p) {
      const s = p.sizes[0];
      const iou = p.generated.iou === null ? "-" : p.generated.iou.toFixed(2);
      fig.append(el("figcaption", "muted", t("result.info", { size: s.size, stroke: s.checks.metrics.outlinePx.toFixed(1), alpha: t(`result.alpha.${p.post.alphaSource}`), iou })));
    }
    box.append(fig);
  }
  if (p) box.append(renderChecks(p.sizes[0].checks, p.generated.report));
  const opts = el("div", "make-row");
  const outline = el("select", "make-cli");
  outline.setAttribute("aria-label", t("result.outline"));
  for (const v of ["code", "none", "model"]) outline.append(new Option(t(`result.outline.${v}`), v));
  outline.value = g.outline;
  outline.disabled = !!g.busy;
  outline.addEventListener("change", () => hooks.postOption("outline", outline.value));
  const bg = el("select", "make-cli");
  bg.setAttribute("aria-label", t("result.background"));
  for (const v of ["auto", "key"]) bg.append(new Option(t(`result.background.${v}`), v));
  bg.value = g.background;
  bg.disabled = !!g.busy;
  bg.addEventListener("change", () => hooks.postOption("background", bg.value));
  opts.append(el("span", "muted", t("result.outline")), outline, el("span", "muted", t("result.background")), bg);
  const actions = el("div", "body-actions");
  const again = el("button", "act-btn", g.busy === "post" ? t("result.posting") : t("result.repost"));
  again.type = "button";
  again.disabled = !!g.busy;
  again.title = t("result.repostHint");
  again.addEventListener("click", () => hooks.repost());
  const out = el("button", "act-btn primary", g.busy === "export" ? t("result.exporting") : t("result.export"));
  out.type = "button";
  out.disabled = !!g.busy || !p;
  out.title = t("result.exportHint");
  out.addEventListener("click", () => hooks.exportIcon(item));
  actions.append(again, out);
  box.append(opts, actions);
  const x = g.exported;
  if (x?.ok) {
    const line = el("p", "body-hint");
    const open = el("button", "link-btn", t("result.exported", { n: x.files.length, folder: x.folder }));
    open.type = "button";
    open.addEventListener("click", () => hooks.openPath(x.folder));
    line.append(open);
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

function captureButton(items: PrefabItem[], cap: CaptureView, hooks: DetailsHooks, label: string): HTMLButtonElement {
  const b = el("button", "act-btn primary", cap.running ? t("capture.running") : label);
  b.type = "button";
  b.disabled = cap.running || items.length === 0;
  b.addEventListener("click", () => hooks.capture(items));
  return b;
}

/** Status lines under the capture button: progress, or why the last run did not go through. */
function captureNotes(cap: CaptureView): HTMLElement[] {
  const out: HTMLElement[] = [];
  if (cap.running && cap.progress) out.push(el("p", "body-hint", cap.progress));
  else if (cap.error) out.push(el("p", "body-hint warn-text", captureErrorText(cap.error)));
  return out;
}

function previewBox(item: PrefabItem, cap: CaptureView): HTMLElement {
  const box = el("figure", "capture-preview");
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

function one(item: PrefabItem, hooks: DetailsHooks, view: DetailsView): HTMLElement[] {
  const cap = view.capture;
  const head = el("div", "details-head");
  head.append(el("h2", "details-title", item.name));
  const path = el("button", "link-btn path", item.path);
  path.type = "button";
  path.title = t("details.copyPath");
  path.addEventListener("click", () => hooks.copy(item.path));

  const kind = el("select", "kind-select");
  kind.setAttribute("aria-label", t("details.kind"));
  const auto = new Option(t("details.kindAuto"), "");
  kind.append(auto, ...KIND_CHIPS.map((k) => new Option(t(`kind.${k}`), k)));
  kind.value = item.kindSource === "override" ? item.kind : "";
  kind.addEventListener("change", () => hooks.setKind(item, kind.value || null));
  const kindBox = el("div", "kind-box");
  kindBox.append(el("span", "kind-now", t(`kind.${item.kind}`)), kind);

  const cls = item.class === "empty" || item.class === "unknown" ? `class.${item.class}` : `class.${item.class}`;
  const rows: [string, string | Node][] = [
    [t("details.path"), path],
    [t("details.class"), t(cls)],
    [t("details.kind"), kindBox],
    [t("details.kindWhy"), kindSourceText(item)],
    [t("details.nested"), String(item.nested)],
    [t("details.guid"), item.guid || t("details.noGuid")],
  ];
  const actions = el("div", "body-actions");
  const ok = capturable(item.class);
  if (ok) actions.append(captureButton([item], cap, hooks, cap.images[item.path] ? t("capture.retake") : t("capture.take")));
  const open = el("button", "act-btn", t("details.openFolder"));
  open.type = "button";
  open.addEventListener("click", () => hooks.openFolder(item.folder));
  actions.append(open);
  const out: HTMLElement[] = [head];
  if (ok) out.push(previewBox(item, cap));
  out.push(facts(rows), actions, ...captureNotes(cap));
  if (ok) out.push(makeSection(item, view, hooks));
  if (!ok) out.push(el("p", "body-hint warn-text", t("details.not3d")));
  return out;
}

function many(items: PrefabItem[], hooks: DetailsHooks, cap: CaptureView): HTMLElement[] {
  const counts = new Map<string, number>();
  for (const i of items) counts.set(i.kind, (counts.get(i.kind) ?? 0) + 1);
  const head = el("div", "details-head");
  head.append(el("h2", "details-title", t("details.many", { n: items.length })));
  const rows: [string, string][] = KIND_CHIPS.filter((k) => counts.has(k)).map((k) => [t(`kind.${k}`), String(counts.get(k))]);
  const targets = items.filter((i) => capturable(i.class));
  const actions = el("div", "body-actions");
  actions.append(captureButton(targets, cap, hooks, t("capture.takeMany", { n: targets.length })));
  const strip = el("div", "capture-strip");
  for (const i of targets) {
    const src = cap.images[i.path];
    if (!src) continue;
    const img = el("img");
    img.src = src;
    img.alt = i.name;
    img.title = i.name;
    img.draggable = false;
    strip.append(img);
  }
  const out: HTMLElement[] = [head, facts(rows), actions, ...captureNotes(cap)];
  if (strip.childElementCount) out.push(strip);
  return out;
}

export function renderDetails(content: HTMLElement, view: DetailsView, hooks: DetailsHooks): string {
  if (!view.project) {
    content.replaceChildren(readiness(view.ready, hooks));
    return "";
  }
  if (view.picked.length === 1) {
    content.replaceChildren(...one(view.picked[0], hooks, view));
    return view.picked[0].name;
  }
  if (view.picked.length > 1) {
    content.replaceChildren(...many(view.picked, hooks, view.capture));
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
