import { describe, expect, it } from "vitest";
import rust from "../crates/uis-core/src/settings.rs?raw";
import {
  defaultSettings,
  getSetting,
  normalizeSetting,
  patchSettings,
  readSettings,
  receiveSettings,
  onSettingsChange,
  setSettingsSaver,
  SETTINGS,
  whenSaved,
  SETTING_KEYS,
  type SettingDef,
} from "./settings";

/** `pub const NAME: &[&str] = &["a", "b"];` from settings.rs */
function rustList(name: string): string[] {
  const m = new RegExp(`pub const ${name}: &\\[&str\\] = &\\[([^\\]]*)\\]`).exec(rust);
  if (!m) throw new Error(`${name} not in settings.rs`);
  return [...m[1].matchAll(/"([^"]*)"/g)].map((x) => x[1]);
}

/** `clamp(&mut <path>, min, max)` from Settings::normalize */
function rustRange(field: string): [number, number] {
  const m = new RegExp(`clamp\\(&mut [\\w.]*\\b${field}, (\\d+), (\\d+)\\)`).exec(rust);
  if (!m) throw new Error(`no clamp for ${field}`);
  return [Number(m[1]), Number(m[2])];
}

describe("settings schema matches crates/uis-core/src/settings.rs", () => {
  it("has the same choices", () => {
    const options = (key: keyof typeof SETTINGS) => (SETTINGS[key] as SettingDef & { options: { value: string }[] }).options.map((o) => o.value);
    expect(options("language")).toEqual(rustList("LANGUAGES"));
    expect(options("theme")).toEqual(rustList("THEMES"));
    expect(options("capture.mode")).toEqual(rustList("CAPTURE_MODES"));
    expect(options("llm.rewriter")).toEqual(rustList("REWRITERS"));
    expect(options("claude.effort")).toEqual(rustList("CLAUDE_EFFORTS"));
    expect(options("generate.outline")).toEqual(rustList("OUTLINES"));
    expect(options("generate.background")).toEqual(rustList("BACKGROUNDS"));
    expect(options("llm.fidelityReview")).toEqual(rustList("FIDELITY_REVIEWERS"));
    expect(options("generate.fidelity")).toEqual(["strict", "stylized"]);
    expect(rust).toContain(`fidelity: "${SETTINGS["generate.fidelity"].default}".into()`);
    expect(rust).toContain(`fidelity_review: "${SETTINGS["llm.fidelityReview"].default}".into()`);
    const r = SETTINGS["generate.fidelityRetries"] as SettingDef & { min: number; max: number };
    expect([r.min, r.max]).toEqual(rustRange("fidelity_retries"));
    expect(rust).toContain(`fidelity_retries: ${r.default},`);
  });

  it("has the same ranges and defaults", () => {
    const def = SETTINGS["capture.batchTimeoutMin"];
    expect([def.min, def.max]).toEqual(rustRange("batch_timeout_min"));
    expect(rust).toContain(`batch_timeout_min: ${def.default},`);
    expect(rust).toContain(`export_name: "${SETTINGS.exportName.default}".into()`);
    expect(rust).toContain(`restore_tracked_files: ${SETTINGS["capture.restoreTrackedFiles"].default},`);
    for (const [key, field] of [["codex.imageTimeoutSec", "image_timeout_sec"], ["codex.textTimeoutSec", "text_timeout_sec"]] as const) {
      const d = SETTINGS[key];
      expect([d.min, d.max]).toEqual(rustRange(field));
      expect(rust).toContain(`${field}: ${d.default},`);
    }
    expect(rust).toContain(`ignore_user_config: ${SETTINGS["codex.ignoreUserConfig"].default},`);
    expect(rust).toContain(`trim_context: ${SETTINGS["codex.trimContext"].default},`);
    expect(rust).toContain(`clean_generated_images: ${SETTINGS["codex.cleanGeneratedImages"].default},`);
    expect(rust).toContain(`model: "${SETTINGS["claude.model"].default}".into()`);
    for (const [key, field] of [["generate.candidates", "candidates"], ["thumbnails.maxPerRun", "max_per_run"]] as const) {
      const d = SETTINGS[key];
      expect([d.min, d.max]).toEqual(rustRange(field));
      expect(rust).toMatch(new RegExp(`\\b${field}: ${d.default}\\b`));
    }
    expect(rust).toContain(`style_refs: ${SETTINGS["generate.styleRefs"].default},`);
  });
});

describe("number items", () => {
  it("have a default on their step grid (an off-grid value blocks the popup's form)", () => {
    for (const def of Object.values(SETTINGS) as SettingDef[]) {
      if (def.kind !== "number") continue;
      expect((def.default - def.min) % def.step, def.label).toBe(0);
    }
  });
});

describe("reading", () => {
  it("takes nested values by dotted key and fits them", () => {
    const values = readSettings({ theme: "dark", workDir: null, capture: { mode: "batch", batchTimeoutMin: 999 } });
    expect(values.theme).toBe("dark");
    expect(values.workDir).toBe("");
    expect(values["capture.mode"]).toBe("batch");
    expect(values["capture.batchTimeoutMin"]).toBe(240);
  });

  it("falls back to the default for unknown choices and wrong kinds", () => {
    const values = readSettings({ language: "fr", capture: { restoreTrackedFiles: "yes" } });
    expect(values.language).toBe("auto");
    expect(values["capture.restoreTrackedFiles"]).toBe(true);
    expect(Object.keys(values).sort()).toEqual([...SETTING_KEYS].sort());
  });

  it("an empty name pattern goes back to its default", () => {
    expect(normalizeSetting(SETTINGS.exportName, "   ")).toBe("icon_{prefab}");
    expect(normalizeSetting(SETTINGS.exportDir, "  ")).toBe("");
  });
});

describe("saving", () => {
  it("sends only the changed keys, unset folders as null", async () => {
    const sent: unknown[] = [];
    setSettingsSaver(async (patch) => {
      sent.push(patch);
      return null;
    });
    receiveSettings(defaultSettings());
    await patchSettings({ theme: "light", language: "auto" });
    await patchSettings({ workDir: "C:\\fake\\work" });
    await patchSettings({ workDir: "" });
    expect(sent).toEqual([{ theme: "light" }, { workDir: "C:\\fake\\work" }, { workDir: "" }]);
    expect(getSetting("theme")).toBe("light");
  });

  it("listeners hear a change at once; whenSaved waits for the file", async () => {
    let written = false;
    setSettingsSaver(async () => {
      await new Promise((r) => setTimeout(r, 20));
      written = true;
      return null;
    });
    receiveSettings(defaultSettings());
    const heard: boolean[] = [];
    onSettingsChange(() => heard.push(written));
    void patchSettings({ "codex.path": "C:\fake\codex.exe" });
    expect(heard).toEqual([false]);
    await whenSaved();
    expect(written).toBe(true);
  });
});
