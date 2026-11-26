// @vitest-environment jsdom
import { beforeAll, describe, expect, it } from "vitest";
import ko from "./i18n/ko.json";
import en from "./i18n/en.json";
import checksRs from "../crates/uis-core/src/checks.rs?raw";
import fidelityRs from "../crates/uis-core/src/fidelity.rs?raw";
import { CHECK_GROUPS, rangeText, renderChecks, valueText } from "./components/checks-table";
import { exportErrorText } from "./screens/details";
import { setLang } from "./i18n";
import type { CheckItem, CheckReport } from "./types";

beforeAll(() => setLang("en"));

const item = (id: string, group: string, value: number, min: number | null, max: number | null, pass = true): CheckItem => ({ id, group, value, min, max, pass });

describe("checks table", () => {
  it("names every check and group of checks.rs in both languages", () => {
    const ids = [...new Set([...checksRs.matchAll(/item\("(\w+)", "(\w+)"/g)].map((m) => m[1]))];
    const groups = [...new Set([...checksRs.matchAll(/item\("\w+", "(\w+)"/g)].map((m) => m[1]))];
    expect(ids.length).toBeGreaterThan(15);
    expect([...CHECK_GROUPS].sort()).toEqual([...groups, "fidelity"].sort());
    const fid = [...new Set([...fidelityRs.matchAll(/(?:item\("|id: ")(\w+)"/g)].map((m) => m[1]))];
    expect(fid.sort()).toEqual(["deltaSaturation", "deltaValue", "fidelityProfile", "foreignBlobs", "outsideParts"]);
    for (const id of [...ids, ...fid]) {
      expect(ko).toHaveProperty(`checks.item.${id}`);
      expect(en).toHaveProperty(`checks.item.${id}`);
    }
    for (const g of CHECK_GROUPS) expect(en).toHaveProperty(`checks.group.${g}`);
    // Review findings (review.rs kinds) and fidelity choices
    for (const k of ["added", "resized", "missing", "pattern"]) {
      expect(ko).toHaveProperty(`review.kind.${k}`);
      expect(en).toHaveProperty(`review.kind.${k}`);
    }
    for (const f of ["strict", "stylized"]) expect(ko).toHaveProperty(`make.fidelity.${f}.hint`);
  });

  it("shows values, ranges and failures", () => {
    expect(valueText(item("outlineWidth", "outline", 3.712, 3, 5))).toBe("3.71");
    expect(rangeText(item("outlineWidth", "outline", 3.7, 3, 5))).toBe("3–5");
    expect(rangeText(item("innerDark", "color", 0.01, null, 0.06))).toBe("≤ 0.06");
    expect(valueText(item("nativeAlpha", "generate", 1, 1, null))).toBe("yes");
    const final: CheckReport = {
      pass: false,
      failed: ["saturation"],
      outlineOnly: false,
      items: [item("size", "frame", 0.88, 0.82, 0.995), item("saturation", "color", 0.66, 0.35, 0.62, false)],
      metrics: { outlinePx: 3.7, outlineColor: "#753719" },
    };
    const raw: CheckReport = { ...final, items: [item("iou", "generate", 0.9, 0.8, null), item("saturation", "color", 0.66, 0.35, 0.62, false)] };
    const box = renderChecks(final, raw);
    expect(box.querySelector(".checks-head")!.textContent).toBe("1 check(s) did not pass");
    expect(box.querySelectorAll("tr.bad")).toHaveLength(1);
    // Raw-picture rows: only the generate group (its color rows repeat the icon's)
    expect([...box.querySelectorAll("th")].map((th) => th.textContent)).toEqual(["Frame", "Color", "Raw picture"]);
  });

  it("explains a refused export", () => {
    expect(exportErrorText("insideProject:C:/Game")).toContain("C:/Game");
    expect(exportErrorText("noExportDir")).toContain("export folder");
  });
});
