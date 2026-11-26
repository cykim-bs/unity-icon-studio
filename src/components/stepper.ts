/**
 * The step row of a run: ① capture ② prompt ③ rewrite ④ generate ⑤ finish ⑥ check, each with its
 * state (waiting, running, done, failed, skipped, stopped) and its time. The running step's time
 * is drawn again every second by the page (data-start holds when it began).
 */

import { t } from "../i18n";
import { STEPS, stepSeconds, type ItemRunView } from "../run-state";

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className = "", text = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
}

const MARKS: Record<string, string> = { wait: "", run: "", ok: "✓", fail: "!", skip: "–", cancel: "×" };

export function renderStepper(item: ItemRunView, now: number): HTMLElement {
  const row = el("ol", "stepper");
  row.setAttribute("aria-label", t("steps.label"));
  STEPS.forEach((id, i) => {
    const s = item.steps[id];
    const li = el("li", `step step-${s.state}`);
    li.dataset.step = id;
    li.dataset.state = s.state;
    const num = el("span", "step-num", s.state === "wait" || s.state === "run" ? String(i + 1) : MARKS[s.state]);
    const name = el("span", "step-name", t(`steps.${id}`));
    const time = el("span", "step-time", stepSeconds(s, now));
    if (s.state === "run" && s.startMs !== null) time.dataset.start = String(s.startMs);
    li.title = `${t(`steps.${id}`)} — ${t(`steps.state.${s.state}`)}`;
    li.append(num, name, time);
    row.append(li);
  });
  return row;
}

/** Updates the running steps' times in place (once a second while a run goes). */
export function tickSteppers(root: ParentNode, now: number): void {
  for (const time of root.querySelectorAll<HTMLElement>(".step-time[data-start]")) {
    const start = Number(time.dataset.start);
    time.textContent = stepSeconds({ state: "run", startMs: start, endMs: null }, now);
  }
}
