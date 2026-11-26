/// <reference types="vite/client" />
import { describe, expect, it } from "vitest";
import tokensCss from "./theme/tokens.css?raw";
import darkCss from "./theme/dark.css?raw";
import baseCss from "./theme/base.css?raw";
import bootJs from "../public/theme-boot.js?raw";
import { PREFS_PREFIX } from "./prefs";
import { COLOR_TOKEN_NAMES, oppositeChoice, resolveBase, THEME_MIRROR_KEY, themeTransitionCss } from "./theme";

/** `--name: #hex;` declarations of the first block after `selector`. */
function block(css: string, selector: string): Record<string, string> {
  const start = css.indexOf(selector);
  expect(start, selector).toBeGreaterThanOrEqual(0);
  const open = css.indexOf("{", start);
  const close = css.indexOf("}", open);
  const out: Record<string, string> = {};
  for (const m of css.slice(open + 1, close).matchAll(/--([\w-]+):\s*(#[0-9a-fA-F]{6})\s*;/g)) out[m[1]] = m[2].toLowerCase();
  return out;
}

function luminance(hex: string): number {
  const [r, g, b] = [1, 3, 5].map((i) => {
    const s = parseInt(hex.slice(i, i + 2), 16) / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((p, q) => q - p);
  return (hi + 0.05) / (lo + 0.05);
}

const channels = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));

/** color-mix(in srgb, b t, a) */
function mix(a: string, b: string, t: number): string {
  const [x, y] = [channels(a), channels(b)];
  return "#" + x.map((v, i) => Math.round(v + (y[i] - v) * t).toString(16).padStart(2, "0")).join("");
}

const isGray = (hex: string) => new Set(channels(hex)).size === 1;

const light = block(tokensCss, ":root {");
const darkMedia = block(darkCss, ':root:not([data-theme="light"]) {');
const dark = block(darkCss, ':root[data-theme="dark"] {');

describe("theme tokens", () => {
  it("define every registered token, solid, in both themes", () => {
    const names = COLOR_TOKEN_NAMES.map((n) => n.slice(2)).sort();
    expect(Object.keys(light).sort()).toEqual(names);
    expect(Object.keys(dark).sort()).toEqual(names);
  });

  it("keep the system-dark block and the pinned-dark block identical", () => {
    expect(darkMedia).toEqual(dark);
  });

  it("keep the neutral palette: gray chrome, one pink accent", () => {
    expect(light).toMatchObject({ fg: "#323232", bg: "#fdfdfd", accent: "#d12c6a", "sidebar-bg": "#f2f2f2" });
    expect(dark).toMatchObject({ fg: "#e8e8e8", bg: "#212121", accent: "#ff6b9e", "sidebar-bg": "#2c2c2c" });
    for (const p of [light, dark]) {
      for (const k of ["fg", "bg", "muted", "line", "sidebar-bg"]) expect(isGray(p[k]), k).toBe(true);
    }
  });

  for (const [name, p] of [
    ["light", light],
    ["dark", dark],
  ] as const) {
    it(`reach 4.5:1 for text in ${name}`, () => {
      const pairs: [string, string][] = [];
      for (const text of ["fg", "muted", "warn", "hot", "info", "batch"]) {
        for (const back of ["bg", "sidebar-bg", "tint-warn", "tint-hot"]) pairs.push([text, back]);
      }
      pairs.push(["accent", "bg"], ["on-accent", "accent"], ["on-danger", "danger"], ["on-caption-close", "caption-close"]);
      const low = pairs
        .map(([a, b]) => ({ pair: `${a} on ${b}`, ratio: contrast(p[a], p[b]) }))
        .filter((x) => x.ratio < 4.5);
      expect(low).toEqual([]);
    });

    it(`reach 3:1 for status marks in ${name}`, () => {
      for (const mark of ["ok", "warn", "hot", "batch", "accent"]) {
        for (const back of ["bg", "sidebar-bg"]) expect(contrast(p[mark], p[back]), `${mark} on ${back}`).toBeGreaterThanOrEqual(3);
      }
    });

    it(`reach 3:1 for the focus ring in ${name}`, () => {
      const m = /--ring:\s*color-mix\(in srgb, var\(--fg\) (\d+)%, var\(--bg\)\)/.exec(baseCss);
      expect(m, "--ring in base.css").not.toBeNull();
      const ring = mix(p.bg, p.fg, Number(m![1]) / 100);
      for (const back of ["bg", "sidebar-bg"]) expect(contrast(ring, p[back]), `ring on ${back}`).toBeGreaterThanOrEqual(3);
    });
  }
});

describe("theme choice", () => {
  it("follows the system only for 'system'", () => {
    expect(resolveBase("system", true)).toBe("dark");
    expect(resolveBase("system", false)).toBe("light");
    expect(resolveBase("light", true)).toBe("light");
    expect(resolveBase("dark", false)).toBe("dark");
  });

  it("pins the opposite side", () => {
    expect(oppositeChoice("dark")).toBe("light");
    expect(oppositeChoice("light")).toBe("dark");
  });

  it("fades every token", () => {
    const css = themeTransitionCss(250);
    for (const n of COLOR_TOKEN_NAMES) expect(css).toContain(`${n} 250ms`);
  });

  it("is mirrored under the key the boot script reads", () => {
    expect(bootJs).toContain(JSON.stringify(PREFS_PREFIX + THEME_MIRROR_KEY));
  });
});
