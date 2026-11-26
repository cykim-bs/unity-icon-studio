/**
 * The 「Composition」 box of one prefab: how far the camera looks down (pitch), how far it is turned
 * (yaw) and to which side, the front axis, the root rotation (presets for models that come in
 * lying down or facing away, or typed values) and the projection. Each value says where it comes
 * from: the kind's preset, the pack's rule, or a person (kept per project, prefab_override).
 * 「Back to the start」 clears the person's values. A change makes the capture stale: the prompt's
 * CAMERA sentence is built from the values a capture was taken with, so it needs a new capture.
 */

import { t } from "../i18n";
import type { CameraView } from "../types";

export interface CameraHooks {
  /** Stores a person's values (`null` clears a key) */
  set(patch: Record<string, unknown>): void;
  reset(): void;
  retake(): void;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className = "", text = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
}

/** Root rotation presets (Euler degrees). */
export const ROTATIONS: { id: string; value: [number, number, number] }[] = [
  { id: "none", value: [0, 0, 0] },
  { id: "standUp", value: [270, 0, 0] },
  { id: "standUpBack", value: [90, 0, 0] },
  { id: "turnAround", value: [0, 180, 0] },
  { id: "turnRight", value: [0, 90, 0] },
];

export const FRONTS = ["+z", "-z", "+x", "-x"] as const;

const num = (v: unknown, d: number): number => (typeof v === "number" && Number.isFinite(v) ? v : d);
const rot = (v: unknown): [number, number, number] =>
  Array.isArray(v) && v.length === 3 && v.every((x) => typeof x === "number") ? (v as [number, number, number]) : [0, 0, 0];
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/** Where a value comes from: `person`, `rule` or `preset`. */
export function sourceOf(view: CameraView, key: string): "person" | "rule" | "preset" {
  const p = view.person as Record<string, unknown> | null;
  if (p && (p[key] !== undefined || (key === "yaw" && p.yawSign !== undefined))) return "person";
  if (!same(view.rule[key], view.preset[key])) return "rule";
  return "preset";
}

/** The rotation preset of a value, or `custom`. */
export function rotationPreset(r: [number, number, number]): string {
  const norm = (x: number) => ((x % 360) + 360) % 360;
  return ROTATIONS.find((p) => p.value.every((x, i) => norm(x) === norm(r[i])))?.id ?? "custom";
}

const CAMERA_FIELDS = ["yaw", "pitch", "projection", "fov"];

const pickCamera = (v: Record<string, unknown> | null | undefined): Record<string, unknown> | null =>
  v ? Object.fromEntries(CAMERA_FIELDS.filter((k) => v[k] !== undefined).map((k) => [k, v[k]])) : null;

/**
 * The camera the prompt's sentence is built with. With a capture: the values that capture was
 * taken with, even when the composition changed since (the picture sent is that capture until it
 * is taken again). Without one: the composition now (the next capture uses it).
 */
export function promptCamera(hasCapture: boolean, captured: Record<string, unknown> | null | undefined, now: Record<string, unknown> | null | undefined): { values: Record<string, unknown> | null; from: "capture" | "now" } {
  if (hasCapture && captured) return { values: pickCamera(captured), from: "capture" };
  return { values: pickCamera(now), from: "now" };
}

/** True when a person changed anything of the camera (not counting the kind). */
export function hasPersonCamera(view: CameraView | null): boolean {
  const p = view?.person as Record<string, unknown> | null | undefined;
  return !!p && ["yaw", "pitch", "yawSign", "front", "rotation", "projection", "fov"].some((k) => p[k] !== undefined);
}

export function renderCameraBox(view: CameraView | null, stale: boolean, busy: boolean, hooks: CameraHooks): HTMLElement {
  const box = el("section", "camera-box");
  box.id = "camera-box";
  box.tabIndex = -1;
  const head = el("div", "camera-head");
  head.append(el("h3", "make-title", t("camera.title")));
  if (!view) {
    box.append(head, el("p", "muted", t("camera.loading")));
    return box;
  }
  const reset = el("button", "link-btn", t("camera.reset"));
  reset.type = "button";
  reset.id = "cam-reset";
  reset.disabled = busy || !hasPersonCamera(view);
  reset.addEventListener("click", () => hooks.reset());
  head.append(reset);
  box.append(head);

  const e = view.effective;
  const grid = el("div", "camera-grid");
  const row = (key: string, label: string, ...controls: HTMLElement[]) => {
    grid.append(el("label", "camera-label", label));
    const cell = el("div", "camera-cell");
    cell.append(...controls);
    grid.append(cell);
    const src = sourceOf(view, key);
    const tag = el("span", `camera-src src-${src}`, t(`camera.src.${src}`));
    tag.dataset.key = key;
    grid.append(tag);
  };
  const number = (id: string, value: number, min: number, max: number, onCommit: (v: number) => void) => {
    const input = el("input", "camera-num");
    input.type = "number";
    input.id = id;
    input.min = String(min);
    input.max = String(max);
    input.step = "1";
    // As the attribute too: a script driving the window can wait for a value with a selector
    input.defaultValue = String(Math.round(value * 10) / 10);
    input.disabled = busy;
    input.addEventListener("change", () => {
      const v = Number(input.value);
      if (Number.isFinite(v) && v >= min && v <= max) onCommit(v);
      else input.value = String(Math.round(value * 10) / 10);
    });
    return input;
  };
  const deg = () => el("span", "muted", "°");

  const yaw = num(e.yaw, 28);
  row("pitch", t("camera.pitch"), number("cam-pitch", num(e.pitch, 30), -10, 90, (v) => hooks.set({ pitch: v })), deg());
  row("yaw", t("camera.yaw"), number("cam-yaw", Math.abs(yaw), 0, 90, (v) => hooks.set({ yaw: v })), deg());
  const flip = el("input");
  flip.type = "checkbox";
  flip.id = "cam-flip";
  flip.checked = yaw < 0;
  flip.disabled = busy;
  // The pack's side is the start: a person's yawSign says which side shows now
  flip.addEventListener("change", () => hooks.set({ yawSign: flip.checked ? -1 : 1 }));
  const flipLabel = el("label", "check-label");
  flipLabel.append(flip, document.createTextNode(t(yaw < 0 ? "camera.sideLeft" : "camera.sideRight")));
  row("yawSign", t("camera.flip"), flipLabel);

  const front = el("select", "make-cli");
  front.id = "cam-front";
  for (const f of FRONTS) front.append(new Option(t(`camera.front.${f}`), f));
  front.value = typeof e.front === "string" ? e.front : "+z";
  front.disabled = busy;
  front.addEventListener("change", () => hooks.set({ front: front.value }));
  row("front", t("camera.frontAxis"), front);

  const r = rot(e.rotation);
  const preset = rotationPreset(r);
  const rsel = el("select", "make-cli");
  rsel.id = "cam-rotation";
  for (const p of ROTATIONS) rsel.append(new Option(t(`camera.rot.${p.id}`), p.id));
  rsel.append(new Option(t("camera.rot.custom"), "custom"));
  rsel.value = preset;
  rsel.disabled = busy;
  const xyz = el("span", "camera-xyz");
  xyz.hidden = preset !== "custom";
  const axes = (["x", "y", "z"] as const).map((a, i) =>
    number(`cam-rot-${a}`, r[i], -360, 360, (v) => {
      const next = [...r] as [number, number, number];
      next[i] = v;
      hooks.set({ rotation: next });
    }),
  );
  axes.forEach((input, i) => xyz.append(el("span", "muted", ["X", "Y", "Z"][i]), input));
  rsel.addEventListener("change", () => {
    if (rsel.value === "custom") {
      xyz.hidden = false;
      return;
    }
    hooks.set({ rotation: ROTATIONS.find((p) => p.id === rsel.value)!.value });
  });
  row("rotation", t("camera.rotation"), rsel, xyz);

  const proj = el("select", "make-cli");
  proj.id = "cam-projection";
  for (const p of ["orthographic", "perspective"]) proj.append(new Option(t(`camera.proj.${p}`), p));
  proj.value = e.projection === "perspective" ? "perspective" : "orthographic";
  proj.disabled = busy;
  proj.addEventListener("change", () => hooks.set({ projection: proj.value }));
  const fov = number("cam-fov", num(e.fov, 20), 5, 90, (v) => hooks.set({ fov: v }));
  const fovBox = el("span", "camera-xyz");
  fovBox.append(el("span", "muted", "FOV"), fov, deg());
  fovBox.hidden = proj.value !== "perspective";
  row("projection", t("camera.projection"), proj, fovBox);
  box.append(grid);

  const chips = el("div", "make-row");
  const chip = (id: string, label: string, patch: Record<string, unknown>) => {
    const b = el("button", "chip-btn small", label);
    b.type = "button";
    b.id = id;
    b.disabled = busy;
    b.addEventListener("click", () => hooks.set(patch));
    return b;
  };
  chips.append(
    el("span", "muted", t("camera.quick")),
    chip("cam-front-view", t("camera.quick.front"), { yaw: 10, pitch: 25 }),
    chip("cam-flip-view", t("camera.quick.flip"), { yawSign: yaw < 0 ? 1 : -1 }),
  );
  box.append(chips);
  if (stale) {
    const note = el("div", "camera-stale");
    note.setAttribute("role", "status");
    note.append(el("span", "warn-text", t("camera.stale")));
    const again = el("button", "act-btn", t("capture.retake"));
    again.type = "button";
    again.id = "cam-retake";
    again.disabled = busy;
    again.addEventListener("click", () => hooks.retake());
    note.append(again);
    box.append(note);
  }
  return box;
}
