/**
 * Line icons for prefab kinds and classes (16x16 paths drawn for this app, plain shapes), used in
 * grid cells while there is no thumbnail.
 */

import { icon } from "./shell/icons";

export const KIND_ICONS: Record<string, string> = {
  // a house: roof and walls with a door
  building: "M2.5 7.25L8 2.75l5.5 4.5M3.75 6.25v7h8.5v-7M6.75 13.25v-3.5h2.5v3.5",
  // a box with a lid line
  prop: "M2.75 5.25L8 2.75l5.25 2.5v5.5L8 13.25l-5.25-2.5zM2.75 5.25L8 7.75l5.25-2.5M8 7.75v5.5",
  // head and shoulders
  character: "M8 2.75a2.5 2.5 0 1 1 0 5 2.5 2.5 0 0 1 0-5zM3.25 13.25c.5-2.6 2.4-4 4.75-4s4.25 1.4 4.75 4",
  // a cube seen from a corner
  generic: "M8 2.25l5.25 3v5.5L8 13.75l-5.25-3v-5.5zM2.75 5.25L8 8.25l5.25-3M8 8.25v5.5",
};

export const CLASS_ICONS: Record<string, string> = {
  ui: "M2.25 3.25h11.5v9.5H2.25zM4.5 6h7M4.5 8.5h4.5",
  "2d": "M2.75 2.75h10.5v10.5H2.75zM2.75 10.5l3.5-3.5 3 3 1.5-1.5 2.5 2.5",
  fx: "M8 1.75v3M8 11.25v3M1.75 8h3M11.25 8h3M3.6 3.6l2.1 2.1M10.3 10.3l2.1 2.1M3.6 12.4l2.1-2.1M10.3 5.7l2.1-2.1",
  other: "M3.25 3.25h9.5v9.5h-9.5zM3.25 3.25l9.5 9.5",
  model: "M8 2.25l5.25 3v5.5L8 13.75l-5.25-3v-5.5zM2.75 5.25L8 8.25l5.25-3M8 8.25v5.5",
};

/** The icon a cell shows: the kind for 3D items and model files, the class otherwise. */
export function itemIcon(cls: string, kind: string): SVGSVGElement {
  if (cls === "3d" || cls === "model") return icon(KIND_ICONS[kind] ?? KIND_ICONS.generic);
  const key = cls === "empty" || cls === "unknown" ? "other" : cls;
  return icon(CLASS_ICONS[key] ?? CLASS_ICONS.other);
}
