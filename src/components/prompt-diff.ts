/**
 * Before/after of a rewrite: the sentences the note changed or added (with the fixed text they
 * replaced), the parts left out and why, the one-line summary. Locked sentences never change, so
 * they are only counted. An empty note says the fixed prompt goes as it is.
 */

import { t } from "../i18n";
import { failureText, rejectText } from "../prompt-text";
import type { RewriteReport } from "../types";

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className = "", text = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
}

export function renderPromptDiff(r: RewriteReport): HTMLElement {
  const box = el("section", "prompt-diff");
  box.setAttribute("aria-label", t("prompt.diffTitle"));
  if (!r.cli && r.ok) {
    box.append(el("p", "muted", t("prompt.noNote")));
    return box;
  }
  const head = el("p", "prompt-diff-head");
  const secs = (r.calls.reduce((n, c) => n + c.elapsedMs, 0) / 1000).toFixed(1);
  head.textContent = t("prompt.by", { cli: r.cli === "claude" ? "Claude" : "Codex", s: secs, n: r.calls.length });
  box.append(head);
  if (r.fallbackFrom) box.append(el("p", "body-hint", t("prompt.fallback", { from: r.fallbackFrom === "claude" ? "Claude" : "Codex" })));
  if (r.failure) {
    box.append(el("p", "warn-text", failureText(r.failure)));
    return box;
  }
  if (r.diff.summaryKo) box.append(el("p", "prompt-summary", r.diff.summaryKo));
  const changed = r.diff.sentences.filter((s) => s.status !== "same");
  const list = el("ol", "prompt-changes");
  for (const s of changed) {
    const li = el("li", `prompt-change ${s.status}`);
    li.dataset.id = s.id;
    li.append(el("span", "prompt-tag", t(`prompt.status.${s.status}`)), el("span", "prompt-id", s.id));
    if (s.before) li.append(el("del", "prompt-before", s.before));
    li.append(el("ins", "prompt-after", s.text));
    list.append(li);
  }
  if (changed.length) box.append(list);
  else box.append(el("p", "muted", t("prompt.unchanged")));
  if (r.diff.rejected.length) {
    box.append(el("h4", "prompt-sub", t("prompt.rejected")));
    const rej = el("ul", "prompt-rejected");
    for (const x of r.diff.rejected) {
      const li = el("li");
      li.append(el("span", "prompt-part", x.part), el("span", "muted", ` — ${rejectText(x)}`));
      rej.append(li);
    }
    box.append(rej);
  }
  if (r.diff.outline === "off") box.append(el("p", "warn-text", t("prompt.outlineOff")));
  if (r.diff.camera !== "keep") box.append(el("p", "body-hint", t("prompt.cameraHint")));
  const locked = r.diff.sentences.filter((s) => s.locked).length;
  box.append(el("p", "muted", t("prompt.lockedKept", { n: locked })));
  return box;
}
