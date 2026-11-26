/**
 * Right column. Nothing picked: what to do and the readiness list. One prefab: its facts and the
 * kind (a person can change it; kept per project). Several: how many and of which kinds.
 */

import { capturable, captureErrorText, itemErrorText } from "../capture-text";
import { t } from "../i18n";
import { KIND_CHIPS } from "../prefab-filter";
import type { PackSummary, PrefabItem, ProjectSummary } from "../types";
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
}

export interface DetailsView {
  project: ProjectSummary | null;
  pack: PackSummary | null;
  picked: PrefabItem[];
  lang: string;
  capture: CaptureView;
}

export interface DetailsHooks {
  setKind(item: PrefabItem, kind: string | null): void;
  openFolder(folder: string): void;
  copy(text: string): void;
  capture(items: PrefabItem[]): void;
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

function readiness(): HTMLElement {
  const list = el("div", "ready-list");
  list.append(el("h3", "", t("ready.title")));
  for (const key of ["ready.codex", "ready.claude", "ready.unityCli", "ready.unity"]) {
    const row = el("div", "ready-row");
    row.append(el("span", "ready-dot"), el("span", "ready-name", t(key)), el("span", "muted", t("ready.notChecked")));
    list.append(row);
  }
  return list;
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

function one(item: PrefabItem, hooks: DetailsHooks, cap: CaptureView): HTMLElement[] {
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
    content.replaceChildren(readiness());
    return "";
  }
  if (view.picked.length === 1) {
    content.replaceChildren(...one(view.picked[0], hooks, view.capture));
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
  content.replaceChildren(box, summary, readiness());
  return "";
}
