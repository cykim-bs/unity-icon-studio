/**
 * What the screen keeps of a run (one or more prefabs): each item's six steps with their state
 * and times, built from the run's events (uis-core pipeline.rs RunEvent) and settled by its
 * report. No drawing here (components/stepper.ts draws a step row).
 */

import type { RunEvent, RunItem, RunReport } from "./types";

export const STEPS = ["capture", "prompt", "rewrite", "generate", "post", "check"] as const;
export type StepId = (typeof STEPS)[number];
export type StepState = "wait" | "run" | "ok" | "fail" | "skip" | "cancel";

export interface StepView {
  state: StepState;
  startMs: number | null;
  endMs: number | null;
}

export type ItemState = "waiting" | "running" | "ok" | "failed" | "cancelled" | "stopped";

export interface ItemRunView {
  asset: string;
  name: string;
  state: ItemState;
  steps: Record<StepId, StepView>;
  /** The settled item from the report (null while running) */
  result: RunItem | null;
}

export interface RunView {
  running: boolean;
  /** A stop was asked for (the capture finishes its request first) */
  cancelling: boolean;
  startedMs: number;
  items: ItemRunView[];
  /** Capture progress (`stage:done/total`) */
  captureNote: string;
  report: RunReport | null;
  /** Why the call itself failed (`busy`, a code), not an item */
  error: string | null;
}

const stepOf = (stage: string): StepId | null => ((STEPS as readonly string[]).includes(stage) ? (stage as StepId) : null);

function emptySteps(): Record<StepId, StepView> {
  const out = {} as Record<StepId, StepView>;
  for (const s of STEPS) out[s] = { state: "wait", startMs: null, endMs: null };
  return out;
}

export function newRun(items: { asset: string; name: string; captured: boolean }[], now: number): RunView {
  return {
    running: true,
    cancelling: false,
    startedMs: now,
    captureNote: "",
    report: null,
    error: null,
    items: items.map((i) => {
      const steps = emptySteps();
      if (i.captured) steps.capture.state = "skip";
      return { asset: i.asset, name: i.name, state: "waiting", steps, result: null };
    }),
  };
}

function setStep(step: StepView, state: StepState, now: number) {
  if (state === "run") {
    if (step.state !== "run") step.startMs = now;
    step.endMs = null;
  } else {
    step.startMs ??= now;
    step.endMs = now;
  }
  step.state = state;
}

const EVENT_STATE: Record<string, StepState> = { start: "run", ok: "ok", fail: "fail", skip: "skip", cancel: "cancel" };

/** Applies one event. The capture runs for every item that had no picture. */
export function applyEvent(run: RunView, e: RunEvent, now: number): void {
  if (e.stage === "done") {
    run.running = false;
    return;
  }
  if (e.stage === "capture") {
    if (e.state === "progress") {
      run.captureNote = e.message;
      return;
    }
    const state = EVENT_STATE[e.state];
    if (!state) return;
    for (const it of run.items) {
      if (it.steps.capture.state === "skip" && state !== "skip") continue;
      setStep(it.steps.capture, state, now);
    }
    return;
  }
  const step = stepOf(e.stage);
  const it = run.items.find((i) => i.asset === e.item);
  const state = EVENT_STATE[e.state];
  if (!step || !it || !state) return;
  if (it.state === "waiting") it.state = "running";
  // A second candidate starts the picture step again; its time runs from the first start
  if (step === "generate" && state === "run" && it.steps.generate.state === "ok") {
    it.steps.generate.state = "run";
    it.steps.generate.endMs = null;
    return;
  }
  setStep(it.steps[step], state, now);
}

/** Settles every item from the report: states, the failed step, steps left running. */
export function finishRun(run: RunView, report: RunReport, now: number): void {
  run.running = false;
  run.report = report;
  for (const it of run.items) {
    const r = report.items.find((x) => x.asset === it.asset);
    if (!r) continue;
    it.result = r;
    it.state = (["ok", "failed", "cancelled", "stopped"] as const).find((s) => s === r.state) ?? "failed";
    const failed = r.stage ? stepOf(r.stage) : null;
    if (failed && it.state === "failed") setStep(it.steps[failed], "fail", now);
    for (const s of STEPS) {
      const step = it.steps[s];
      if (step.state === "run") setStep(step, it.state === "cancelled" ? "cancel" : it.state === "ok" ? "ok" : "fail", now);
    }
    if (it.state === "cancelled" || it.state === "stopped") {
      for (const s of STEPS) if (it.steps[s].state === "wait") it.steps[s].state = "cancel";
    }
  }
}

/** Seconds a step took (or has taken so far), one decimal under 10 s. */
export function stepSeconds(step: StepView, now: number): string {
  if (step.startMs === null) return "";
  const ms = (step.endMs ?? now) - step.startMs;
  const s = ms / 1000;
  if (s < 10) return `${s.toFixed(1)}s`;
  const m = Math.floor(s / 60);
  return m ? `${m}:${String(Math.floor(s % 60)).padStart(2, "0")}` : `${Math.floor(s)}s`;
}

/** The item a run is working on now, else the first one still waiting. */
export function activeItem(run: RunView): ItemRunView | null {
  return run.items.find((i) => i.state === "running") ?? run.items.find((i) => i.state === "waiting") ?? null;
}

/** Counts for a run of several items. */
export function runCounts(run: RunView): { ok: number; pass: number; failed: number; left: number } {
  let ok = 0;
  let pass = 0;
  let failed = 0;
  let left = 0;
  for (const i of run.items) {
    if (i.state === "ok") {
      ok++;
      if (i.result?.pass) pass++;
    } else if (i.state === "failed") failed++;
    else left++;
  }
  return { ok, pass, failed, left };
}
