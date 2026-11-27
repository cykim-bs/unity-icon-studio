/**
 * Screen texts in Korean and English (i18n/{ko,en}.json; scripts/check-i18n.mjs keeps the keys in step).
 * Adapted from unity-slot-widget src/i18n.ts, MIT.
 */
import ko from "./i18n/ko.json";
import en from "./i18n/en.json";

export type Lang = "ko" | "en";
type Strings = Record<string, string>;

const TABLES: Record<Lang, Strings> = { ko, en };
let lang: Lang = "en";

export function resolveLang(setting: string | undefined): Lang {
  if (setting === "ko" || setting === "en") return setting;
  const nav = (navigator.languages?.[0] ?? navigator.language ?? "en").toLowerCase();
  return nav.startsWith("ko") ? "ko" : "en";
}

export function setLang(next: Lang) {
  lang = next;
  document.documentElement.lang = next;
}

export function currentLang(): Lang {
  return lang;
}

/** `t("status.editors", { open: 3, max: 6 })`. A missing key shows the key itself. */
export function t(key: string, params: Record<string, string | number> = {}): string {
  const text = TABLES[lang][key] ?? TABLES.en[key] ?? key;
  return text.replace(/\{(\w+)\}/g, (_, name: string) => (name in params ? String(params[name]) : `{${name}}`));
}

/**
 * Fills static texts from keys in the markup: `data-t` (text), `data-t-title`, `data-t-aria`
 * (aria-label) and `data-t-placeholder`. scripts/check-i18n.mjs checks that every such key exists.
 */
export function applyDomTexts(root: ParentNode = document): void {
  root.querySelectorAll<HTMLElement>("[data-t]").forEach((el) => (el.textContent = t(el.dataset.t!)));
  root.querySelectorAll<HTMLElement>("[data-t-title]").forEach((el) => (el.title = t(el.dataset.tTitle!)));
  root.querySelectorAll<HTMLElement>("[data-t-aria]").forEach((el) => el.setAttribute("aria-label", t(el.dataset.tAria!)));
  root
    .querySelectorAll<HTMLInputElement>("[data-t-placeholder]")
    .forEach((el) => (el.placeholder = t(el.dataset.tPlaceholder!)));
}
