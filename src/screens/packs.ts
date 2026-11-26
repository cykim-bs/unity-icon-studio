/**
 * Side panel tab 3 「Styles」: the style packs (the built-in one and local ones in the tool's
 * folder). The project's pack can be picked here. For the pack shown: its fixed prompt for a kind,
 * sentence by sentence, locked ones marked (a note can only change the unlocked ones), how many
 * style references and kind rules it has, and a button that opens its folder.
 */

import { t } from "../i18n";
import { KIND_CHIPS } from "../prefab-filter";
import type { Assembled, PackSummary } from "../types";

export interface PacksView {
  packs: PackSummary[];
  /** The pack the open project uses (null: no project) */
  projectPack: string | null;
  /** The pack shown */
  shown: string;
  kind: string;
  prompt: Assembled | null;
  lang: string;
}

export interface PacksHooks {
  show(id: string): void;
  use(id: string): void;
  kind(kind: string): void;
  openFolder(dir: string): void;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className = "", text = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
}

export function packLabel(p: PackSummary, lang: string): string {
  const n = p.name;
  if (typeof n === "string") return n;
  if (n && typeof n === "object") return n[lang] ?? n.en ?? p.id;
  return p.id;
}

export function renderPacks(body: HTMLElement, view: PacksView, hooks: PacksHooks): void {
  if (!view.packs.length) {
    body.replaceChildren(el("p", "panel-empty", t("packs.empty")));
    return;
  }
  const list = el("ul", "pack-list");
  for (const p of view.packs) {
    const li = el("li", "pack-row");
    const b = el("button", "run-item pack-item");
    b.type = "button";
    b.dataset.pack = p.id;
    if (p.id === view.shown) b.classList.add("on");
    const text = el("span", "run-text");
    text.append(el("span", "run-title", packLabel(p, view.lang)), el("span", "run-sub muted", `${p.id} · ${t(`packs.source.${p.source}`)}${p.extends ? ` · ${t("packs.extends", { id: p.extends })}` : ""}`));
    b.append(text);
    if (p.id === view.projectPack) b.append(el("span", "run-badge ok", t("packs.inUse")));
    if (p.problem) b.append(el("span", "run-badge bad", t("packs.problem")));
    b.title = p.problem ?? "";
    b.addEventListener("click", () => hooks.show(p.id));
    li.append(b);
    list.append(li);
  }
  const out: HTMLElement[] = [list];
  const shown = view.packs.find((p) => p.id === view.shown) ?? view.packs[0];
  const box = el("section", "pack-detail");
  box.append(el("h3", "make-title", packLabel(shown, view.lang)));
  const facts = el("p", "muted", t("packs.facts", { refs: shown.styleRefs, rules: shown.kindRules }));
  box.append(facts);
  const actions = el("div", "body-actions");
  if (view.projectPack !== null && view.projectPack !== shown.id && !shown.problem) {
    const use = el("button", "act-btn", t("packs.use"));
    use.type = "button";
    use.addEventListener("click", () => hooks.use(shown.id));
    actions.append(use);
  }
  if (shown.dir) {
    const open = el("button", "link-btn", t("packs.openFolder"));
    open.type = "button";
    open.addEventListener("click", () => hooks.openFolder(shown.dir!));
    actions.append(open);
  }
  if (actions.childElementCount) box.append(actions);
  const kinds = el("div", "chip-group small");
  kinds.setAttribute("role", "group");
  kinds.setAttribute("aria-label", t("details.kind"));
  for (const k of KIND_CHIPS) {
    const c = el("button", "chip-btn small", t(`kind.${k}`));
    c.type = "button";
    c.setAttribute("aria-pressed", String(view.kind === k));
    c.addEventListener("click", () => hooks.kind(k));
    kinds.append(c);
  }
  box.append(el("h4", "prompt-sub", t("packs.blocks")), kinds);
  if (view.prompt) {
    box.append(el("p", "muted", t("packs.template", { v: view.prompt.templateVersion })));
    const ol = el("ol", "pack-sentences");
    let block = "";
    for (const s of view.prompt.sentences) {
      if (s.block !== block) {
        block = s.block;
        ol.append(el("li", "pack-block", block));
      }
      const li = el("li", s.locked ? "pack-sentence locked" : "pack-sentence");
      li.title = `${s.id} — ${s.locked ? t("packs.locked") : t("packs.open")}`;
      li.append(el("span", "pack-lock", s.locked ? "🔒" : "✎"), el("span", "", s.text));
      ol.append(li);
    }
    box.append(ol, el("p", "body-hint", t("packs.lockHint")));
  }
  out.push(box);
  body.replaceChildren(...out);
}
