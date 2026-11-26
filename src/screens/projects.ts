/**
 * Side panel tab 1: the projects on the list. Each row shows the folder name, the end of its path,
 * a state dot (batch capture, open editor, cannot capture; the reason in the tooltip), the branch
 * and the Unity version. A press picks the project; the right-click menu opens the folder, checks
 * again or takes it off the list.
 */

import { t } from "../i18n";
import type { ProjectSummary } from "../types";

export interface ProjectsView {
  projects: ProjectSummary[];
  current: string | null;
}

export interface ProjectsHooks {
  pick(id: string): void;
  menu(id: string, event: MouseEvent): void;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className = "", text = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
}

/** The last two parts of a path, for a row that has little room. */
export function pathTail(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.length <= 2 ? path : `…\\${parts.slice(-2).join("\\")}`;
}

/** Text for a probe's mode and reason (status bar, tooltips, the header). */
export function modeText(p: Pick<ProjectSummary, "mode" | "reason">): string {
  if (p.mode === "attach") return t("mode.attach");
  if (p.mode === "batch") return t("mode.batch");
  return t("mode.blocked");
}

export function reasonText(p: Pick<ProjectSummary, "mode" | "reason">): string {
  if (p.reason.startsWith("notProject")) return t("reason.notProject");
  switch (p.reason) {
    case "open":
      return t("reason.open");
    case "closed":
      return t("reason.closed");
    case "noPipelinePackage":
      return t("reason.noPipelinePackage");
    case "noVersion":
      return t("reason.noVersion");
    default:
      return p.reason;
  }
}

export function renderProjects(body: HTMLElement, view: ProjectsView, hooks: ProjectsHooks): void {
  if (!view.projects.length) {
    body.replaceChildren(el("p", "panel-empty", t("projects.none")));
    return;
  }
  const list = el("ul", "project-list");
  list.setAttribute("role", "listbox");
  list.setAttribute("aria-label", t("nav.projects"));
  for (const p of view.projects) {
    const row = el("li", "project-row");
    row.setAttribute("role", "option");
    row.tabIndex = 0;
    row.dataset.id = p.id;
    const selected = p.id === view.current;
    row.setAttribute("aria-selected", String(selected));
    if (selected) row.classList.add("selected");
    const dot = el("span", `state-dot mode-${p.exists ? p.mode : "blocked"}`);
    const head = el("div", "project-head");
    head.append(dot, el("span", "project-name", p.name || pathTail(p.path)));
    const sub = el("div", "project-sub");
    const facts = [p.exists ? modeText(p) : t("projects.missing")];
    if (p.branch) facts.push(p.branch);
    if (p.unityVersion) facts.push(p.unityVersion);
    sub.textContent = facts.join(" · ");
    row.title = `${p.path}\n${p.exists ? reasonText(p) : t("projects.missing")}`;
    row.append(head, sub);
    row.addEventListener("click", () => hooks.pick(p.id));
    row.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        hooks.pick(p.id);
      }
    });
    row.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      hooks.menu(p.id, e);
    });
    list.append(row);
  }
  body.replaceChildren(list);
}
