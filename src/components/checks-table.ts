/**
 * The pixel checks of a finished icon (and of the raw picture before it), grouped: frame,
 * outline, color, raw picture. Each row: name, value, range, pass or fail.
 */

import { t } from "../i18n";
import type { CheckItem, CheckReport } from "../types";

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className = "", text = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
}

export const CHECK_GROUPS = ["frame", "outline", "color", "generate", "fidelity"] as const;

/** Items that are yes/no (1 = yes): shown as a word, not a number. */
const FLAGS = new Set(["canvas", "notTouching", "nativeAlpha", "noModelOutline"]);

export function valueText(item: CheckItem): string {
  if (FLAGS.has(item.id)) return item.value >= 1 ? t("checks.yes") : t("checks.no");
  const v = item.value;
  return Math.abs(v) >= 10 ? v.toFixed(0) : v.toFixed(2);
}

export function rangeText(item: CheckItem): string {
  if (FLAGS.has(item.id)) return "";
  const f = (v: number) => (Math.abs(v) >= 10 ? v.toFixed(0) : String(Math.round(v * 1000) / 1000));
  if (item.min !== null && item.max !== null) return `${f(item.min)}–${f(item.max)}`;
  if (item.min !== null) return `≥ ${f(item.min)}`;
  if (item.max !== null) return `≤ ${f(item.max)}`;
  return "";
}

/** The table for `final` (the icon), `raw` (the generated picture's checks) and `fidelity` (its
 * checks against the capture). */
export function renderChecks(final: CheckReport, raw: CheckReport | null, fidelity: CheckReport | null = null): HTMLElement {
  const box = el("section", "checks-table");
  box.setAttribute("aria-label", t("checks.title"));
  const items = [...final.items, ...(raw?.items.filter((i) => i.group === "generate") ?? []), ...(fidelity?.items ?? [])];
  const failed = items.filter((i) => !i.pass).length;
  const head = el("p", failed === 0 ? "checks-head ok" : "checks-head bad");
  head.textContent = failed === 0 ? t("checks.pass") : t("checks.fail", { n: failed });
  box.append(head);
  const table = el("table");
  for (const group of CHECK_GROUPS) {
    const rows = items.filter((i) => i.group === group);
    if (!rows.length) continue;
    const gh = el("tr", "checks-group");
    const th = el("th", "", t(`checks.group.${group}`));
    th.colSpan = 3;
    gh.append(th);
    table.append(gh);
    for (const i of rows) {
      const tr = el("tr", i.pass ? "" : "bad");
      const name = el("td");
      name.append(el("span", `ready-dot ${i.pass ? "ok" : "bad"}`), el("span", "", t(`checks.item.${i.id}`)));
      tr.append(name, el("td", "num", valueText(i)), el("td", "muted", rangeText(i)));
      table.append(tr);
    }
  }
  box.append(table);
  return box;
}
