import { beforeAll, describe, expect, it } from "vitest";
import ko from "./i18n/ko.json";
import en from "./i18n/en.json";
import rust from "../crates/uis-core/src/capture.rs?raw";
import { capturable, captureErrorText, itemErrorText } from "./capture-text";
import { setLang } from "./i18n";

beforeAll(() => setLang("en"));

describe("capture texts", () => {
  it("turns run error codes into sentences", () => {
    expect(captureErrorText("blocked:noPipelinePackage")).toContain("com.unity.pipeline");
    expect(captureErrorText("blocked:editorBusy:compiling")).toContain("compiling");
    expect(captureErrorText("hookRefused:leased by someone")).toContain("leased by someone");
    expect(captureErrorText("editorMissing:6000.0.1f1:6000.0.2f1,6000.1.0f1")).toBe("Unity 6000.0.1f1 is not installed (found: 6000.0.2f1,6000.1.0f1)");
    expect(captureErrorText("editorMissing:6000.0.1f1:")).toContain("found: -");
    expect(captureErrorText("batchExit:4")).toContain("exit code 4");
    expect(captureErrorText("attach:compile:CS0103 x")).toContain("compile:CS0103 x");
    expect(captureErrorText("weird")).toContain("weird");
  });

  it("has a sentence for every plain code the core returns", () => {
    for (const code of ["entryExists", "batchPending", "batchTimeout", "noItems"]) {
      expect(rust).toContain(`"${code}"`);
      expect(captureErrorText(code)).not.toContain(code);
    }
  });

  it("names item errors and keeps unknown ones", () => {
    expect(itemErrorText("prefab_not_found")).toBe("prefab not found");
    expect(itemErrorText("exception: Boom")).toBe("exception: Boom");
    expect(capturable("3d") && capturable("model") && !capturable("ui")).toBe(true);
  });

  it("keeps the Korean and English tables in step", () => {
    const k = Object.keys(ko).filter((x) => x.startsWith("capture.") || x.startsWith("recovery."));
    const e = Object.keys(en).filter((x) => x.startsWith("capture.") || x.startsWith("recovery."));
    expect(k.sort()).toEqual(e.sort());
  });
});
