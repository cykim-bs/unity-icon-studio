/**
 * Words for capture results: the error codes of uis-core capture.rs (`blocked:<reason>`,
 * `hookRefused:<line>`, `editorMissing:<version>:<found>`, ...) and the per-item errors the Unity
 * script writes (`prefab_not_found`, `ui_prefab`, ...).
 */

import { t } from "./i18n";

const BLOCKED = ["open", "noPipelinePackage", "notResponding", "closedAttachOnly", "openBatchOnly", "noVersion", "unityCliMissing"];
const ITEM = ["prefab_not_found", "ui_prefab", "no_renderers", "instantiate_failed", "empty_image", "uniform_image"];
const PLAIN = ["entryExists", "batchPending", "batchTimeout", "noItems"];

/** Text for a run's error code. */
export function captureErrorText(code: string): string {
  const [head, ...rest] = code.split(":");
  const tail = rest.join(":");
  if (head === "blocked") {
    if (tail.startsWith("editorBusy")) return t("capture.err.editorBusy", { state: tail.split(":")[1] ?? "" });
    if (tail.startsWith("unityCli:")) return t("capture.err.unityCli", { text: tail.slice("unityCli:".length) });
    const reason = tail.split(":")[0];
    return BLOCKED.includes(reason) ? t(`capture.blocked.${reason}`) : t("capture.err.other", { text: code });
  }
  if (head === "hookRefused") return t("capture.err.hookRefused", { text: tail });
  if (head === "hookFailed") return t("capture.err.hookFailed", { text: tail });
  if (head === "editorMissing") return t("capture.err.editorMissing", { version: rest[0] ?? "", found: rest.slice(1).join(":") || "-" });
  if (head === "unityCliMissing") return t("capture.blocked.unityCliMissing");
  if (PLAIN.includes(head)) return t(`capture.err.${head}`);
  if (head === "batchExit") return t("capture.err.batchExit", { code: tail });
  if (head === "attach") return t("capture.err.attach", { text: tail });
  return t("capture.err.other", { text: code });
}

/** Text for an item error written by the capture script. */
export function itemErrorText(code: string): string {
  return ITEM.includes(code) ? t(`capture.item.${code}`) : code;
}

/** True when the item can be captured (3D prefabs and model files). */
export function capturable(cls: string): boolean {
  return cls === "3d" || cls === "model";
}
