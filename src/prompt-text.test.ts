// @vitest-environment jsdom
import { beforeAll, describe, expect, it } from "vitest";
import ko from "./i18n/ko.json";
import en from "./i18n/en.json";
import errorsRs from "../crates/uis-core/src/cli/errors.rs?raw";
import promptRs from "../crates/uis-core/src/prompt.rs?raw";
import { renderPromptDiff } from "./components/prompt-diff";
import { setLang } from "./i18n";
import { FAIL_KINDS, REJECT_CODES, failureText, generateErrorText, readyRow, rejectText } from "./prompt-text";
import type { Readiness, RewriteReport } from "./types";

beforeAll(() => setLang("en"));

describe("prompt texts", () => {
  it("has a sentence for every failure kind of cli/errors.rs, in both languages", () => {
    const rust = [...errorsRs.matchAll(/FailKind::\w+ => "(\w+)"/g)].map((m) => m[1]);
    expect([...FAIL_KINDS].sort()).toEqual([...new Set(rust)].sort());
    for (const k of FAIL_KINDS) {
      expect(ko).toHaveProperty(`llm.err.${k}`);
      expect(en).toHaveProperty(`llm.err.${k}`);
    }
    expect(failureText({ kind: "limit", cli: "codex", message: "usage limit", exitCode: 1 })).toBe("Codex usage limit reached: usage limit");
    expect(failureText({ kind: "weird", cli: "claude", message: "x", exitCode: 1 })).toBe("Claude failed: x");
    expect(generateErrorText("rewrite:model:no such model")).toContain("no such model");
  });

  it("names the reasons the tool leaves a part out", () => {
    for (const code of REJECT_CODES) expect(promptRs).toContain(`"${code}:`);
    expect(rejectText({ part: "x", reason: "locked:OUTPUT.spec", by: "code" })).toBe("locked sentence (OUTPUT.spec)");
    expect(rejectText({ part: "x", reason: "forbidden:logo", by: "code" })).toContain("logo");
    expect(rejectText({ part: "x", reason: "no backgrounds", by: "model" })).toBe("no backgrounds");
  });

  it("readiness rows", () => {
    const cli = { found: true, path: "C:\\x\\codex.exe", source: "copy", version: "codex-cli 1", loggedIn: true, imageTool: true, enabled: true, problem: null };
    const ready: Readiness = { codex: cli, claude: { ...cli, imageTool: null }, unityCli: cli, codexApp: null, codexCopyNeeded: false, elapsedMs: 1 };
    expect(readyRow("codex", null).level).toBe("unknown");
    expect(readyRow("codex", ready)).toEqual({ level: "ok", text: "codex-cli 1" });
    expect(readyRow("claude", { ...ready, claude: { ...ready.claude, loggedIn: false } }).level).toBe("bad");
    expect(readyRow("codex", { ...ready, codex: { ...cli, imageTool: false } }).level).toBe("warn");
    const app = { version: "2", resources: "x", bytes: 1 };
    expect(readyRow("codex", { ...ready, codex: { ...cli, found: false }, codexApp: app }).text).toBe("Copy from the desktop app first");
    expect(readyRow("claude", { ...ready, claude: { ...cli, enabled: false } }).level).toBe("warn");
  });
});

function report(over: Partial<RewriteReport>): RewriteReport {
  return {
    ok: true,
    note: "winter",
    cli: "claude",
    fallbackFrom: null,
    finalText: "",
    calls: [{ cli: "claude", purpose: "rewrite", attempt: 1, elapsedMs: 4200, code: "ok" }],
    failure: null,
    choices: [],
    diff: {
      changed: true,
      outline: "keep",
      camera: "keep",
      summaryKo: "summary",
      rejected: [{ part: "the logo", reason: "forbidden:logo", by: "code" }],
      sentences: [
        { id: "HEADER.verbatim", block: "HEADER", locked: true, text: "Use this prompt.", status: "same" },
        { id: "STYLE.color", block: "STYLE", locked: false, text: "Color: winter.", status: "replaced", before: "Color: bright." },
        { id: "SUBJECT.add1", block: "SUBJECT", locked: false, text: "Snow on the roof.", status: "added" },
      ],
    },
    ...over,
  };
}

describe("prompt diff panel", () => {
  it("shows changed and added sentences, what was left out and the locked count", () => {
    const box = renderPromptDiff(report({}));
    const items = [...box.querySelectorAll(".prompt-change")].map((li) => (li as HTMLElement).dataset.id);
    expect(items).toEqual(["STYLE.color", "SUBJECT.add1"]);
    expect(box.querySelector(".prompt-before")?.textContent).toBe("Color: bright.");
    expect(box.querySelector(".prompt-after")?.textContent).toBe("Color: winter.");
    expect(box.querySelector(".prompt-rejected")?.textContent).toContain("forbidden by the output rules (logo)");
    expect(box.textContent).toContain("1 locked sentences kept");
    expect(box.textContent).toContain("Claude · 4.2 s · 1 call(s)");
  });

  it("an empty note and a failure", () => {
    expect(renderPromptDiff(report({ cli: null, calls: [] })).textContent).toContain("No note");
    const failed = renderPromptDiff(report({ ok: false, failure: { kind: "auth", cli: "claude", message: "expired", exitCode: 1 } }));
    expect(failed.querySelector(".warn-text")?.textContent).toContain("needs a sign-in");
    expect(failed.querySelector(".prompt-change")).toBeNull();
    const fb = renderPromptDiff(report({ cli: "codex", fallbackFrom: "claude" }));
    expect(fb.textContent).toContain("Claude could not be used");
  });

  it("keeps the Korean and English tables in step", () => {
    const pick = (o: object) => Object.keys(o).filter((x) => /^(prompt|make|llm|codex|ready)\./.test(x)).sort();
    expect(pick(ko)).toEqual(pick(en));
  });
});
