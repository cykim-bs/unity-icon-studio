// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { renderStepper } from "./components/stepper";
import { setLang } from "./i18n";
import { activeItem, applyEvent, finishRun, newRun, runCounts, stepSeconds, STEPS } from "./run-state";
import { jobBadge } from "./screens/runs";
import { atText, runErrorText } from "./screens/details";
import type { JobSummary, RunEvent, RunItem, RunReport } from "./types";

const ev = (stage: string, state: string, item: string | null = null, message = ""): RunEvent => ({ runId: "r", stage, state, item, index: 1, total: 2, message, elapsedMs: 0 });

function item(asset: string, state: string, extra: Partial<RunItem> = {}): RunItem {
  return {
    asset,
    name: asset,
    kind: "prop",
    capture: null,
    jobDir: null,
    state,
    stage: null,
    error: null,
    rewrite: null,
    candidates: [],
    best: null,
    pass: false,
    outlineKept: false,
    export: null,
    elapsedMs: 0,
    ...extra,
  };
}

function report(items: RunItem[], extra: Partial<RunReport> = {}): RunReport {
  return { schema: "", runId: "r", project: "", ok: false, cancelled: false, stopped: null, error: null, capture: null, items, elapsedMs: 0, ...extra };
}

describe("run state from events", () => {
  it("walks the six steps of an item", () => {
    const run = newRun([{ asset: "A", name: "A", captured: false }], 0);
    applyEvent(run, ev("capture", "start"), 100);
    expect(run.items[0].steps.capture.state).toBe("run");
    applyEvent(run, ev("capture", "progress", null, "unity:1/1"), 150);
    expect(run.captureNote).toBe("unity:1/1");
    applyEvent(run, ev("capture", "ok"), 1100);
    expect(run.items[0].steps.capture).toEqual({ state: "ok", startMs: 100, endMs: 1100 });
    for (const [stage, state] of [["prompt", "start"], ["prompt", "ok"], ["rewrite", "skip"], ["generate", "start"]]) applyEvent(run, ev(stage, state, "A"), 1200);
    expect(run.items[0].state).toBe("running");
    expect(activeItem(run)?.asset).toBe("A");
    expect(run.items[0].steps.rewrite.state).toBe("skip");
    expect(run.items[0].steps.generate.state).toBe("run");
    applyEvent(run, ev("generate", "ok", "A"), 50_000);
    applyEvent(run, ev("post", "start", "A"), 50_000);
    applyEvent(run, ev("post", "ok", "A"), 51_000);
    applyEvent(run, ev("check", "fail", "A"), 51_000);
    expect(run.items[0].steps.check.state).toBe("fail");
    expect(stepSeconds(run.items[0].steps.generate, 0)).toBe("48s");
    applyEvent(run, ev("done", "fail"), 51_000);
    expect(run.running).toBe(false);
  });

  it("keeps the capture step of items that had a picture", () => {
    const run = newRun([{ asset: "A", name: "A", captured: true }, { asset: "B", name: "B", captured: false }], 0);
    applyEvent(run, ev("capture", "start"), 10);
    expect(run.items[0].steps.capture.state).toBe("skip");
    expect(run.items[1].steps.capture.state).toBe("run");
  });

  it("a second candidate keeps the picture step running from its first start", () => {
    const run = newRun([{ asset: "A", name: "A", captured: true }], 0);
    applyEvent(run, ev("generate", "start", "A"), 1000);
    applyEvent(run, ev("generate", "ok", "A"), 2000);
    applyEvent(run, ev("generate", "start", "A"), 2500);
    expect(run.items[0].steps.generate).toEqual({ state: "run", startMs: 1000, endMs: null });
  });

  it("the report settles failures, stops and steps left running", () => {
    const run = newRun([{ asset: "A", name: "A", captured: true }, { asset: "B", name: "B", captured: true }], 0);
    applyEvent(run, ev("generate", "start", "A"), 10);
    finishRun(run, report([item("A", "failed", { stage: "generate", error: "image:limit" }), item("B", "stopped")], { stopped: "limit" }), 20);
    expect(run.items[0].state).toBe("failed");
    expect(run.items[0].steps.generate.state).toBe("fail");
    expect(run.items[1].state).toBe("stopped");
    expect(run.items[1].steps.prompt.state).toBe("cancel");
    expect(runCounts(run)).toEqual({ ok: 0, pass: 0, failed: 1, left: 1 });
  });

  it("times: tenths under ten seconds, then seconds and minutes", () => {
    expect(stepSeconds({ state: "ok", startMs: 0, endMs: 2340 }, 0)).toBe("2.3s");
    expect(stepSeconds({ state: "run", startMs: 0, endMs: null }, 75_000)).toBe("1:15");
    expect(stepSeconds({ state: "wait", startMs: null, endMs: null }, 5)).toBe("");
  });
});

describe("step row", () => {
  it("draws the six steps with their state and a live time on the running one", () => {
    setLang("en");
    const run = newRun([{ asset: "A", name: "A", captured: true }], 0);
    applyEvent(run, ev("prompt", "ok", "A"), 5);
    applyEvent(run, ev("generate", "start", "A"), 1000);
    const row = renderStepper(run.items[0], 4000);
    const steps = [...row.querySelectorAll<HTMLElement>(".step")];
    expect(steps.map((s) => s.dataset.step)).toEqual([...STEPS]);
    expect(steps.map((s) => s.dataset.state)).toEqual(["skip", "ok", "wait", "run", "wait", "wait"]);
    expect(steps[3].querySelector<HTMLElement>(".step-time")!.dataset.start).toBe("1000");
    expect(steps[3].textContent).toContain("Generate");
    expect(steps[3].textContent).toContain("3.0s");
  });
});

describe("runs list and texts", () => {
  const job = (extra: Partial<JobSummary>): JobSummary => ({
    dir: "d",
    at: "20261126-183005",
    name: "Crate",
    kind: "prop",
    packId: "default",
    note: "",
    project: null,
    projectName: null,
    asset: null,
    ok: true,
    state: "ok",
    pass: true,
    failedStage: null,
    error: null,
    candidates: [1],
    posted: [1],
    best: 1,
    icon: null,
    exported: false,
    ...extra,
  });

  it("badges: exported, passed, checks failed, failed", () => {
    expect(jobBadge(job({ exported: true })).key).toBe("runs.badge.exported");
    expect(jobBadge(job({})).level).toBe("ok");
    expect(jobBadge(job({ pass: false })).key).toBe("runs.badge.checks");
    expect(jobBadge(job({ state: "failed", posted: [], ok: false })).level).toBe("bad");
    expect(jobBadge(job({ state: "cancelled", posted: [] })).key).toBe("runs.badge.cancelled");
  });

  it("when, in short", () => {
    expect(atText("20261126-183005")).toBe("11-26 18:30");
    expect(atText("odd")).toBe("odd");
  });

  it("run error codes in words", () => {
    setLang("en");
    expect(runErrorText("image:limit")).toContain("limit");
    expect(runErrorText("prefab_not_found")).not.toContain("prefab_not_found:");
    expect(runErrorText("blocked:open")).not.toBe("blocked:open");
  });
});
