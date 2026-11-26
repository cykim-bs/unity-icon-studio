// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { hasPersonCamera, renderCameraBox, rotationPreset, sourceOf } from "./components/camera-box";
import { renderPromptDiff } from "./components/prompt-diff";
import { setLang } from "./i18n";
import type { CameraView, RewriteReport } from "./types";

const preset = { front: "+z", yaw: 28, pitch: 30, projection: "orthographic", fov: 20 };

function view(extra: Partial<CameraView> = {}): CameraView {
  return { preset, rule: { ...preset }, person: null, effective: { ...preset }, ...extra };
}

function hooks() {
  return { set: vi.fn(), reset: vi.fn(), retake: vi.fn() };
}

describe("composition box", () => {
  it("shows the values a capture uses and where each comes from", () => {
    setLang("en");
    const v = view({
      rule: { ...preset, front: "-z" },
      person: { pitch: 45, yawSign: -1 },
      effective: { ...preset, front: "-z", pitch: 45, yaw: -28 },
    });
    const box = renderCameraBox(v, false, false, hooks());
    expect(box.querySelector<HTMLInputElement>("#cam-pitch")!.value).toBe("45");
    expect(box.querySelector<HTMLInputElement>("#cam-yaw")!.value).toBe("28");
    expect(box.querySelector<HTMLInputElement>("#cam-flip")!.checked).toBe(true);
    expect(box.querySelector<HTMLSelectElement>("#cam-front")!.value).toBe("-z");
    const src = (k: string) => box.querySelector<HTMLElement>(`.camera-src[data-key="${k}"]`)!.className;
    expect(src("pitch")).toContain("src-person");
    expect(src("yawSign")).toContain("src-person");
    expect(src("yaw")).toContain("src-person");
    expect(sourceOf(v, "yaw")).toBe("person");
    expect(src("front")).toContain("src-rule");
    expect(src("projection")).toContain("src-preset");
    expect(box.querySelector<HTMLButtonElement>("#cam-reset")!.disabled).toBe(false);
  });

  it("stores what a person changes, and 「back to the start」 only when there is something", () => {
    setLang("en");
    const h = hooks();
    const box = renderCameraBox(view(), false, false, h);
    expect(box.querySelector<HTMLButtonElement>("#cam-reset")!.disabled).toBe(true);
    const pitch = box.querySelector<HTMLInputElement>("#cam-pitch")!;
    pitch.value = "40";
    pitch.dispatchEvent(new Event("change"));
    expect(h.set).toHaveBeenLastCalledWith({ pitch: 40 });
    pitch.value = "200";
    pitch.dispatchEvent(new Event("change"));
    expect(h.set).toHaveBeenCalledTimes(1);
    expect(pitch.value).toBe("30");
    const flip = box.querySelector<HTMLInputElement>("#cam-flip")!;
    flip.checked = true;
    flip.dispatchEvent(new Event("change"));
    expect(h.set).toHaveBeenLastCalledWith({ yawSign: -1 });
    const rot = box.querySelector<HTMLSelectElement>("#cam-rotation")!;
    rot.value = "standUp";
    rot.dispatchEvent(new Event("change"));
    expect(h.set).toHaveBeenLastCalledWith({ rotation: [270, 0, 0] });
    const proj = box.querySelector<HTMLSelectElement>("#cam-projection")!;
    proj.value = "perspective";
    proj.dispatchEvent(new Event("change"));
    expect(h.set).toHaveBeenLastCalledWith({ projection: "perspective" });
    box.querySelector<HTMLButtonElement>("#cam-front-view")!.click();
    expect(h.set).toHaveBeenLastCalledWith({ yaw: 10, pitch: 25 });
  });

  it("typed rotation and the FOV show only when they apply", () => {
    setLang("en");
    const box = renderCameraBox(view({ person: { rotation: [10, 20, 30] }, effective: { ...preset, rotation: [10, 20, 30] } }), false, false, hooks());
    expect(box.querySelector<HTMLSelectElement>("#cam-rotation")!.value).toBe("custom");
    expect(box.querySelector<HTMLElement>("#cam-rot-x")!.parentElement!.hidden).toBe(false);
    expect(box.querySelector<HTMLElement>("#cam-fov")!.parentElement!.hidden).toBe(true);
    expect(rotationPreset([-90, 360, 0])).toBe("standUp");
    expect(rotationPreset([0, 0, 0])).toBe("none");
    expect(hasPersonCamera(view({ person: { kind: "prop" } }))).toBe(false);
  });

  it("a stale capture says so and offers capturing again", () => {
    setLang("en");
    const h = hooks();
    const box = renderCameraBox(view(), true, false, h);
    box.querySelector<HTMLButtonElement>("#cam-retake")!.click();
    expect(h.retake).toHaveBeenCalled();
    expect(renderCameraBox(view(), false, false, h).querySelector("#cam-retake")).toBeNull();
  });

  it("a note that asks for another camera leads to the composition", () => {
    setLang("en");
    const r = {
      ok: true,
      note: "show it from the front",
      cli: "claude",
      fallbackFrom: null,
      finalText: "",
      calls: [{ cli: "claude", purpose: "rewrite", attempt: 1, elapsedMs: 10, code: "ok" }],
      failure: null,
      choices: [],
      diff: { changed: false, sentences: [], rejected: [], summaryKo: "", outline: "keep", camera: "front" },
    } as unknown as RewriteReport;
    const onCamera = vi.fn();
    const box = renderPromptDiff(r, onCamera);
    box.querySelector<HTMLButtonElement>("#cam-hint")!.click();
    expect(onCamera).toHaveBeenCalledWith("front");
    expect(renderPromptDiff(r).querySelector("#cam-hint")).toBeNull();
  });
});
