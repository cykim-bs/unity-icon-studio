/**
 * What the prefab grid shows: find text, class and kind chips, a folder. Pure functions (tested in
 * prefab-filter.test.ts); src/screens/prefabs.ts draws the result.
 */

import type { Kind, PrefabClass, PrefabItem } from "./types";

/** Class chips in order; `other` stands for `empty` and `unknown`. */
export const CLASS_CHIPS = ["3d", "ui", "2d", "fx", "other"] as const;
export type ClassChip = (typeof CLASS_CHIPS)[number];
export const KIND_CHIPS: readonly Kind[] = ["building", "prop", "character", "generic"];

export interface PrefabFilter {
  text: string;
  /** Classes shown; the default is 3D only */
  classes: ClassChip[];
  /** Kinds shown; empty = every kind */
  kinds: Kind[];
  /** Folder prefix (`Assets/Props`); empty = everywhere */
  folder: string;
}

export const DEFAULT_FILTER: PrefabFilter = { text: "", classes: ["3d"], kinds: [], folder: "" };

export function chipOf(cls: PrefabClass): ClassChip | "model" {
  if (cls === "empty" || cls === "unknown") return "other";
  return cls;
}

function inFolder(path: string, folder: string): boolean {
  if (!folder) return true;
  const f = folder.toLowerCase();
  return path.toLowerCase().startsWith(f.endsWith("/") ? f : `${f}/`);
}

/** Every word of the find text in the name or the path (case-insensitive). */
export function matchesText(item: PrefabItem, text: string): boolean {
  const words = text.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return true;
  const hay = `${item.name}\n${item.path}`.toLowerCase();
  return words.every((w) => hay.includes(w));
}

export function applyFilter(items: readonly PrefabItem[], filter: PrefabFilter): PrefabItem[] {
  return items.filter((item) => {
    const chip = chipOf(item.class);
    // model files show with the 3D chip
    const classOk = chip === "model" ? filter.classes.includes("3d") : filter.classes.includes(chip);
    return (
      classOk &&
      (!filter.kinds.length || filter.kinds.includes(item.kind)) &&
      inFolder(item.path, filter.folder) &&
      matchesText(item, filter.text)
    );
  });
}

export interface FolderChoice {
  folder: string;
  count: number;
}

/**
 * Folders to pick from: each folder of the items cut to at most `depth` levels under the project,
 * with how many items are inside, sorted by path.
 */
export function folderChoices(items: readonly PrefabItem[], depth = 4): FolderChoice[] {
  const counts = new Map<string, number>();
  for (const item of items) {
    const parts = item.folder.split("/").filter(Boolean);
    for (let n = 2; n <= Math.min(depth, parts.length); n++) {
      const key = parts.slice(0, n).join("/");
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
  }
  return [...counts.entries()].map(([folder, count]) => ({ folder, count })).sort((a, b) => a.folder.localeCompare(b.folder));
}

/** Item counts per class chip and per kind, for the chip labels. */
export function chipCounts(items: readonly PrefabItem[]): { classes: Record<string, number>; kinds: Record<string, number> } {
  const classes: Record<string, number> = {};
  const kinds: Record<string, number> = {};
  for (const item of items) {
    const chip = chipOf(item.class);
    const key = chip === "model" ? "3d" : chip;
    classes[key] = (classes[key] ?? 0) + 1;
    kinds[item.kind] = (kinds[item.kind] ?? 0) + 1;
  }
  return { classes, kinds };
}

/**
 * Selection after a click on `key` in `order` (the keys on screen): a plain click picks one,
 * Ctrl toggles, Shift takes the range from the anchor (Ctrl+Shift adds the range).
 */
export function nextSelection(
  order: readonly string[],
  selected: ReadonlySet<string>,
  anchor: string | null,
  key: string,
  mods: { ctrl: boolean; shift: boolean },
): { selected: Set<string>; anchor: string } {
  if (mods.shift && anchor !== null && order.includes(anchor)) {
    const a = order.indexOf(anchor);
    const b = order.indexOf(key);
    const [from, to] = a < b ? [a, b] : [b, a];
    const range = new Set(mods.ctrl ? selected : []);
    for (const k of order.slice(from, to + 1)) range.add(k);
    return { selected: range, anchor };
  }
  if (mods.ctrl) {
    const next = new Set(selected);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    return { selected: next, anchor: key };
  }
  return { selected: new Set([key]), anchor: key };
}
