/**
 * Screen texts for the prompt step: why an LLM run failed (cli/errors.rs kinds), why a part of
 * a note was left out (prompt.rs reasons), and the readiness rows. Every kind has a key in both
 * languages (prompt-text.test.ts checks).
 */

import { t } from "./i18n";
import type { CliState, Failure, Readiness, Rejected } from "./types";

export const FAIL_KINDS = [
  "timeout",
  "limit",
  "auth",
  "model",
  "no_image_tool",
  "refused",
  "no_image",
  "bad_json",
  "spawn",
  "not_found",
  "exit",
] as const;

export const REJECT_CODES = ["locked", "unknownId", "notAddable", "forbidden", "banned"] as const;

/** One line for a failed run, with what the CLI said. */
export function failureText(f: Failure): string {
  const kind = (FAIL_KINDS as readonly string[]).includes(f.kind) ? f.kind : "exit";
  const cli = f.cli === "claude" ? "Claude" : "Codex";
  return t(`llm.err.${kind}`, { cli, text: f.message });
}

/** A generate or rewrite error code (`rewrite:<kind>:<message>`, `pack:…`, `badKind:…`). */
export function generateErrorText(code: string): string {
  if (code.startsWith("rewrite:")) {
    const rest = code.slice("rewrite:".length);
    const i = rest.indexOf(":");
    const kind = i < 0 ? rest : rest.slice(0, i);
    return failureText({ kind, cli: "", message: i < 0 ? "" : rest.slice(i + 1), exitCode: null });
  }
  if (code === "busy") return t("error.busy");
  return t("error.other", { text: code });
}

/** Why a part of the note was not applied: the model's own reason, or the tool's code in words. */
export function rejectText(r: Rejected): string {
  if (r.by !== "code") return r.reason;
  const i = r.reason.indexOf(":");
  const code = i < 0 ? r.reason : r.reason.slice(0, i);
  const what = i < 0 ? "" : r.reason.slice(i + 1);
  return (REJECT_CODES as readonly string[]).includes(code) ? t(`prompt.reject.${code}`, { what }) : r.reason;
}

export type ReadyLevel = "ok" | "warn" | "bad" | "unknown";

/** A readiness row: level and text. */
export function readyRow(name: "codex" | "claude" | "unityCli", ready: Readiness | null): { level: ReadyLevel; text: string } {
  if (!ready) return { level: "unknown", text: t("ready.notChecked") };
  const s: CliState = ready[name];
  if (name === "claude" && !s.enabled) return { level: "warn", text: t("ready.off") };
  if (!s.found) {
    if (name === "codex" && ready.codexApp) return { level: "warn", text: t("ready.copyFromApp") };
    return { level: name === "unityCli" ? "warn" : "bad", text: t("ready.missing") };
  }
  if (s.loggedIn === false) return { level: "bad", text: t("ready.notLoggedIn") };
  if (name === "codex" && s.imageTool === false) return { level: "warn", text: t("ready.noImageTool") };
  if (name === "codex" && ready.codexCopyNeeded) return { level: "warn", text: t("ready.newerApp", { version: s.version ?? "" }) };
  return { level: "ok", text: s.version ?? t("ready.found") };
}
