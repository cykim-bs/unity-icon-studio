/**
 * Side panel tab 2 「Runs」: recent job folders, newest first. A row shows the finished icon (when
 * there is one), the prefab, the project, when, and a state badge (passed, checks failed, failed,
 * exported). A press opens the job on the right (export again, finish again, run again).
 */

import { t } from "../i18n";
import type { JobSummary } from "../types";
import { atText } from "./details";

export interface RunsView {
  jobs: JobSummary[];
  /** Icons by job folder (data: URLs) */
  icons: Record<string, string>;
  opened: string | null;
  loading: boolean;
}

export interface RunsHooks {
  open(dir: string): void;
  refresh(): void;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className = "", text = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
}

/** The badge of a job: exported, passed, checks failed, failed. */
export function jobBadge(j: JobSummary): { key: string; level: "ok" | "warn" | "bad" | "info" } {
  if (j.exported) return { key: "runs.badge.exported", level: "info" };
  if (j.state === "ok" || j.posted.length) return j.pass ? { key: "runs.badge.pass", level: "ok" } : { key: "runs.badge.checks", level: "warn" };
  if (j.state === "cancelled" || j.state === "stopped") return { key: "runs.badge.cancelled", level: "warn" };
  return { key: "runs.badge.failed", level: "bad" };
}

export function renderRuns(body: HTMLElement, view: RunsView, hooks: RunsHooks): void {
  if (!view.jobs.length) {
    body.replaceChildren(el("p", "panel-empty", view.loading ? t("runs.loading") : t("runs.empty")));
    return;
  }
  const list = el("ul", "runs-list");
  list.setAttribute("aria-label", t("nav.runs"));
  for (const j of view.jobs) {
    const li = el("li");
    const row = el("button", "run-item");
    row.type = "button";
    row.dataset.dir = j.dir;
    if (j.dir === view.opened) row.classList.add("on");
    row.setAttribute("aria-current", String(j.dir === view.opened));
    const well = el("span", "run-thumb");
    const src = view.icons[j.dir];
    if (src) {
      const img = el("img");
      img.src = src;
      img.alt = "";
      img.draggable = false;
      well.append(img);
    }
    const text = el("span", "run-text");
    text.append(el("span", "run-title", j.name ?? "?"), el("span", "run-sub muted", `${atText(j.at)}${j.projectName ? ` · ${j.projectName}` : ""}`));
    const b = jobBadge(j);
    row.append(well, text, el("span", `run-badge ${b.level}`, t(b.key)));
    row.title = [j.asset, j.note?.trim() ? `${t("make.note")}: ${j.note}` : "", j.error ?? ""].filter(Boolean).join("\n");
    row.addEventListener("click", () => hooks.open(j.dir));
    li.append(row);
    list.append(li);
  }
  const again = el("button", "link-btn", t("runs.refresh"));
  again.type = "button";
  again.addEventListener("click", () => hooks.refresh());
  body.replaceChildren(list, again);
}
