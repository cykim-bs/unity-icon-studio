import { describe, expect, it } from "vitest";
import { applyFilter, chipCounts, DEFAULT_FILTER, folderChoices, matchesText, nextSelection } from "./prefab-filter";
import type { PrefabItem } from "./types";

function item(path: string, cls: PrefabItem["class"], kind: PrefabItem["kind"] = "generic"): PrefabItem {
  const folder = path.slice(0, path.lastIndexOf("/"));
  const name = path.slice(path.lastIndexOf("/") + 1).replace(/\.\w+$/, "");
  return { guid: name, path, name, folder, class: cls, kind, kindSource: "default", skinned: false, nested: 0, mtimeMs: 0 };
}

const ITEMS = [
  item("Assets/Props/Crate.prefab", "3d", "prop"),
  item("Assets/Props/Lamp.prefab", "3d", "prop"),
  item("Assets/Buildings/House.prefab", "3d", "building"),
  item("Assets/Characters/Robot.prefab", "3d", "character"),
  item("Assets/UI/Button.prefab", "ui"),
  item("Assets/FX/Sparkle.prefab", "fx"),
  item("Assets/Misc/Empty.prefab", "empty"),
  item("Assets/Misc/Binary.prefab", "unknown"),
  item("Assets/Models/Robot.fbx", "model", "character"),
];

describe("filter", () => {
  it("shows 3D only by default (model files count as 3D)", () => {
    expect(applyFilter(ITEMS, DEFAULT_FILTER).map((i) => i.name)).toEqual(["Crate", "Lamp", "House", "Robot", "Robot"]);
  });

  it("combines classes, kinds, folder and text", () => {
    expect(applyFilter(ITEMS, { ...DEFAULT_FILTER, classes: ["3d", "ui", "other"] }).length).toBe(8);
    expect(applyFilter(ITEMS, { ...DEFAULT_FILTER, kinds: ["prop", "building"] }).map((i) => i.name)).toEqual(["Crate", "Lamp", "House"]);
    expect(applyFilter(ITEMS, { ...DEFAULT_FILTER, folder: "Assets/Props" }).map((i) => i.name)).toEqual(["Crate", "Lamp"]);
    expect(applyFilter(ITEMS, { ...DEFAULT_FILTER, folder: "Assets/Prop" }).length).toBe(0);
    expect(applyFilter(ITEMS, { ...DEFAULT_FILTER, text: "props LAMP" }).map((i) => i.name)).toEqual(["Lamp"]);
  });

  it("matches every word in the name or the path", () => {
    expect(matchesText(ITEMS[0], "assets crate")).toBe(true);
    expect(matchesText(ITEMS[0], "crate house")).toBe(false);
    expect(matchesText(ITEMS[0], "  ")).toBe(true);
  });

  it("counts per chip and lists folders with counts", () => {
    const c = chipCounts(ITEMS);
    expect(c.classes).toEqual({ "3d": 5, ui: 1, fx: 1, other: 2 });
    expect(c.kinds.prop).toBe(2);
    expect(folderChoices(ITEMS).find((f) => f.folder === "Assets/Misc")?.count).toBe(2);
  });
});

describe("selection", () => {
  const order = ["a", "b", "c", "d", "e"];
  it("plain click picks one, Ctrl toggles, Shift takes a range", () => {
    let s = nextSelection(order, new Set(), null, "b", { ctrl: false, shift: false });
    expect([...s.selected]).toEqual(["b"]);
    s = nextSelection(order, s.selected, s.anchor, "d", { ctrl: false, shift: true });
    expect([...s.selected]).toEqual(["b", "c", "d"]);
    s = nextSelection(order, s.selected, s.anchor, "c", { ctrl: true, shift: false });
    expect([...s.selected].sort()).toEqual(["b", "d"]);
    s = nextSelection(order, s.selected, "e", "a", { ctrl: false, shift: true });
    expect([...s.selected]).toEqual(["a", "b", "c", "d", "e"]);
  });
});
