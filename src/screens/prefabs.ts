/**
 * Middle column: the tools row (find, class chips, kind chips, folder picker, count) and the grid
 * of prefab cells. A cell shows a kind or class icon (no thumbnails yet), the name on up to two
 * lines and a kind badge; its tooltip has the path. Selection: a press picks one, Ctrl+press
 * toggles, Shift+press takes a range; arrows, Home and End move; Enter opens the details.
 */

import { t } from "../i18n";
import { itemIcon } from "../kind-icons";
import { CLASS_CHIPS, KIND_CHIPS, chipCounts, type PrefabFilter } from "../prefab-filter";
import type { PrefabItem } from "../types";

export interface GridView {
  items: PrefabItem[];
  /** Everything the scan found (for chip counts) */
  all: PrefabItem[];
  filter: PrefabFilter;
  selected: ReadonlySet<string>;
  focus: string | null;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className = "", text = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
}

/** The key of an item in the grid and the selection (the path; the guid can be missing). */
export const itemKey = (item: PrefabItem): string => item.path;

export function renderChips(classBox: HTMLElement, kindBox: HTMLElement, view: GridView): void {
  const counts = chipCounts(view.all);
  const chip = (group: string, value: string, label: string, on: boolean, count: number) => {
    const b = el("button", "chip-btn");
    b.type = "button";
    b.dataset.group = group;
    b.dataset.value = value;
    b.setAttribute("aria-pressed", String(on));
    b.textContent = label;
    const n = el("span", "chip-count", String(count));
    b.append(n);
    return b;
  };
  classBox.replaceChildren(
    ...CLASS_CHIPS.map((c) => chip("class", c, t(`class.${c}`), view.filter.classes.includes(c), counts.classes[c] ?? 0)),
  );
  kindBox.replaceChildren(...KIND_CHIPS.map((k) => chip("kind", k, t(`kind.${k}`), view.filter.kinds.includes(k), counts.kinds[k] ?? 0)));
}

export function renderGrid(inner: HTMLElement, view: GridView): void {
  if (!view.items.length) {
    const empty = el("div", "empty-state");
    empty.append(el("p", "muted", t("prefabs.noMatch")));
    inner.replaceChildren(empty);
    return;
  }
  const grid = el("div", "prefab-grid");
  grid.setAttribute("role", "listbox");
  grid.setAttribute("aria-multiselectable", "true");
  grid.setAttribute("aria-label", t("prefabs.label"));
  for (const item of view.items) {
    const key = itemKey(item);
    const cell = el("div", "prefab-cell");
    cell.setAttribute("role", "option");
    cell.dataset.key = key;
    const on = view.selected.has(key);
    cell.setAttribute("aria-selected", String(on));
    if (on) cell.classList.add("selected");
    if (key === view.focus) cell.classList.add("focus");
    cell.title = item.path;
    const well = el("div", "prefab-well");
    well.append(itemIcon(item.class, item.kind));
    const name = el("div", "prefab-name", item.name);
    const badge = el("span", `prefab-badge cls-${item.class}`, item.class === "3d" || item.class === "model" ? t(`kind.${item.kind}`) : t(`class.${item.class === "empty" || item.class === "unknown" ? "other" : item.class}`));
    cell.append(well, name, badge);
    grid.append(cell);
  }
  inner.replaceChildren(grid);
}

/** Columns in the grid as laid out now (for ↑/↓). */
export function gridColumns(inner: HTMLElement): number {
  const cells = inner.querySelectorAll<HTMLElement>(".prefab-cell");
  if (cells.length < 2) return 1;
  const top = cells[0].offsetTop;
  let n = 0;
  for (const c of cells) {
    if (c.offsetTop !== top) break;
    n++;
  }
  return Math.max(1, n);
}
