/**
 * Settings shown in the popup: one entry in `SETTINGS` per key of settings.json, by dotted path
 * (`capture.mode`). Adding an entry makes it appear under its category tab
 * (shell/settings-dialog.ts). Adapted from unity-slot-widget src/settings.ts (itself from
 * https://github.com/cyKim0115/Frond/ @99d89bc), MIT, same author.
 *
 * The values live in settings.json in the tool's home, which a person may edit too, so they come
 * from the Rust side (`settings_get`) and a change is stored by `settings_set(patch)` with only the
 * changed keys; Rust reads the file again, merges and fits the values. A change applies on screen
 * at once and the save runs after it, one at a time. Labels and hints are i18n keys. Ranges and
 * defaults match crates/uis-core/src/settings.rs (settings.test.ts compares).
 */

import { call } from "./backend";

/** Tabs on the left of the popup, in this order. `label` is an i18n key. */
export const SETTING_CATEGORIES = [
  { id: "general", label: "settings.cat.general" },
  { id: "folders", label: "settings.cat.folders" },
  { id: "capture", label: "settings.cat.capture" },
  { id: "generate", label: "settings.cat.generate" },
  { id: "about", label: "settings.cat.about" },
] as const;
export type CategoryId = (typeof SETTING_CATEGORIES)[number]["id"];

interface BaseDef {
  /** The tab this item shows under */
  section: CategoryId;
  /** i18n keys */
  label: string;
  hint?: string;
}
export interface NumberDef extends BaseDef {
  kind: "number";
  default: number;
  min: number;
  max: number;
  /** Spinner step. A whole-number step keeps the value whole (Rust stores an integer). */
  step: number;
  /** i18n key of the unit */
  unit?: string;
  /** Never below this other setting */
  atLeast?: string;
}
export interface SelectOption {
  value: string;
  /** i18n key */
  label: string;
}
export interface SelectDef extends BaseDef {
  kind: "select";
  default: string;
  options: readonly SelectOption[];
}
export interface ToggleDef extends BaseDef {
  kind: "toggle";
  default: boolean;
}
export interface TextDef extends BaseDef {
  kind: "text";
  /** "" stands for an unset value (null in the file) */
  default: string;
  /** i18n key */
  placeholder?: string;
}
export type SettingDef = NumberDef | SelectDef | ToggleDef | TextDef;

export const SETTINGS = {
  language: {
    kind: "select",
    section: "general",
    label: "settings.language",
    default: "auto",
    options: [
      { value: "auto", label: "settings.languageAuto" },
      { value: "ko", label: "lang.ko" },
      { value: "en", label: "lang.en" },
    ],
  },
  theme: {
    kind: "select",
    section: "general",
    label: "settings.theme",
    hint: "settings.theme.hint",
    default: "system",
    options: [
      { value: "system", label: "theme.system" },
      { value: "light", label: "theme.light" },
      { value: "dark", label: "theme.dark" },
    ],
  },
  workDir: { kind: "text", section: "folders", label: "settings.workDir", hint: "settings.workDir.hint", default: "", placeholder: "settings.workDir.placeholder" },
  exportDir: { kind: "text", section: "folders", label: "settings.exportDir", hint: "settings.exportDir.hint", default: "", placeholder: "settings.exportDir.placeholder" },
  exportName: { kind: "text", section: "folders", label: "settings.exportName", hint: "settings.exportName.hint", default: "icon_{prefab}" },
  "capture.mode": {
    kind: "select",
    section: "capture",
    label: "settings.captureMode",
    hint: "settings.captureMode.hint",
    default: "auto",
    options: [
      { value: "auto", label: "settings.captureAuto" },
      { value: "attach", label: "settings.captureAttach" },
      { value: "batch", label: "settings.captureBatch" },
    ],
  },
  "capture.batchTimeoutMin": {
    kind: "number",
    section: "capture",
    label: "settings.batchTimeoutMin",
    hint: "settings.batchTimeoutMin.hint",
    default: 30,
    min: 1,
    max: 240,
    step: 1,
    unit: "unit.mins",
  },
  "capture.restoreTrackedFiles": {
    kind: "toggle",
    section: "capture",
    label: "settings.restoreTrackedFiles",
    hint: "settings.restoreTrackedFiles.hint",
    default: true,
  },
  "llm.rewriter": {
    kind: "select",
    section: "generate",
    label: "settings.rewriter",
    hint: "settings.rewriter.hint",
    default: "claude",
    options: [
      { value: "claude", label: "cli.claude" },
      { value: "codex", label: "cli.codex" },
    ],
  },
  "claude.enabled": { kind: "toggle", section: "generate", label: "settings.claudeEnabled", hint: "settings.claudeEnabled.hint", default: true },
  "claude.model": { kind: "text", section: "generate", label: "settings.claudeModel", hint: "settings.claudeModel.hint", default: "sonnet" },
  "claude.effort": {
    kind: "select",
    section: "generate",
    label: "settings.claudeEffort",
    default: "low",
    options: [
      { value: "low", label: "effort.low" },
      { value: "medium", label: "effort.medium" },
      { value: "high", label: "effort.high" },
    ],
  },
  "codex.path": { kind: "text", section: "generate", label: "settings.codexPath", hint: "settings.codexPath.hint", default: "", placeholder: "settings.codexPath.placeholder" },
  "codex.ignoreUserConfig": { kind: "toggle", section: "generate", label: "settings.ignoreUserConfig", hint: "settings.ignoreUserConfig.hint", default: true },
  "codex.trimContext": { kind: "toggle", section: "generate", label: "settings.trimContext", hint: "settings.trimContext.hint", default: true },
  "codex.cleanGeneratedImages": {
    kind: "toggle",
    section: "generate",
    label: "settings.cleanGeneratedImages",
    hint: "settings.cleanGeneratedImages.hint",
    default: true,
  },
  "codex.imageTimeoutSec": { kind: "number", section: "generate", label: "settings.imageTimeoutSec", default: 300, min: 30, max: 1800, step: 10, unit: "unit.secs" },
  "codex.textTimeoutSec": { kind: "number", section: "generate", label: "settings.textTimeoutSec", default: 90, min: 10, max: 600, step: 5, unit: "unit.secs" },
} satisfies Record<string, SettingDef>;

export type SettingKey = keyof typeof SETTINGS;
export type SettingValue = number | string | boolean;
export type SettingValues = Record<SettingKey, SettingValue>;
export const SETTING_KEYS = Object.keys(SETTINGS) as SettingKey[];

export const isSettingKey = (key: string): key is SettingKey => Object.prototype.hasOwnProperty.call(SETTINGS, key);

/** A stored or typed value made to fit its definition; undefined when it cannot (then: the default). */
export function normalizeSetting(def: SettingDef, raw: unknown): SettingValue | undefined {
  switch (def.kind) {
    case "number": {
      const n = typeof raw === "string" && raw.trim() !== "" ? Number(raw) : raw;
      if (typeof n !== "number" || !Number.isFinite(n)) return undefined;
      const clamped = Math.min(def.max, Math.max(def.min, n));
      return Number.isInteger(def.step) ? Math.round(clamped) : clamped;
    }
    case "select":
      return def.options.some((o) => o.value === raw) ? (raw as string) : undefined;
    case "toggle":
      return typeof raw === "boolean" ? raw : undefined;
    case "text":
      if (raw === null) return "";
      if (typeof raw !== "string") return undefined;
      // a name pattern cannot be empty: back to its default
      return raw.trim() === "" && def.default !== "" ? def.default : raw.trim();
  }
}

export function defaultSettings(): SettingValues {
  return Object.fromEntries(SETTING_KEYS.map((k) => [k, SETTINGS[k].default])) as SettingValues;
}

/** The value at a dotted path of a nested object (what Rust sends). */
export function valueAt(source: unknown, key: string): unknown {
  let node: unknown = source;
  for (const part of key.split(".")) {
    if (typeof node !== "object" || node === null) return undefined;
    node = (node as Record<string, unknown>)[part];
  }
  return node;
}

function crossFix(values: SettingValues): SettingValues {
  const out = { ...values } as Record<string, SettingValue>;
  for (const key of SETTING_KEYS) {
    const def: SettingDef = SETTINGS[key];
    if (def.kind === "number" && def.atLeast) out[key] = Math.max(out[key] as number, out[def.atLeast] as number);
  }
  return out as SettingValues;
}

/** Settings from what Rust sent (nested): each known key fitted over the defaults. */
export function readSettings(raw: unknown): SettingValues {
  const values = defaultSettings() as Record<string, SettingValue>;
  for (const key of SETTING_KEYS) {
    const value = normalizeSetting(SETTINGS[key], valueAt(raw, key));
    if (value !== undefined) values[key] = value;
  }
  return crossFix(values as SettingValues);
}

type Listener = (changed: SettingKey[]) => void;
type Saver = (patch: Partial<Record<SettingKey, unknown>>) => Promise<unknown>;

let values = defaultSettings();
/** The whole document Rust sent last (keys the popup does not show, such as the project list). */
let documentValue: unknown = {};
const listeners = new Set<Listener>();
let saveErrorListener: (error: string) => void = () => {};
let saver: Saver = (patch) => call("settings_set", { patch: toFile(patch) });
const pending = new Map<SettingKey, unknown>();
let queue: Promise<void> = Promise.resolve();

/** Unset text values go to the file as null. */
function toFile(patch: Partial<Record<SettingKey, unknown>>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(patch)) {
    const def = SETTINGS[key as SettingKey] as SettingDef;
    out[key] = def.kind === "text" && value === "" && def.default === "" ? null : value;
  }
  return out;
}

export function getSettings(): SettingValues {
  return values;
}

/** The last settings document from Rust, whole. */
export function settingsDocument(): unknown {
  return documentValue;
}

export function getSetting<K extends SettingKey>(key: K): SettingValues[K] {
  return values[key];
}

export function isDefault(key: SettingKey): boolean {
  return values[key] === SETTINGS[key].default;
}

export function onSettingsChange(listener: Listener): void {
  listeners.add(listener);
}

export function onSettingsSaveError(listener: (error: string) => void): void {
  saveErrorListener = listener;
}

/** Tests: store somewhere else than settings.json. */
export function setSettingsSaver(next: Saver): void {
  saver = next;
}

function commit(next: SettingValues): void {
  const changed = SETTING_KEYS.filter((k) => values[k] !== next[k]);
  values = next;
  if (!changed.length) return;
  for (const listener of listeners) listener(changed);
}

/** Takes settings from the Rust side without saving them. Changes still being saved stay on screen. */
export function receiveSettings(raw: unknown): void {
  if (raw && typeof raw === "object") documentValue = raw;
  const next = readSettings(raw) as Record<string, SettingValue>;
  for (const [key, value] of pending) next[key] = value as SettingValue;
  commit(crossFix(next as SettingValues));
}

/** Applies at once, then stores only the keys that changed (one save at a time). */
export function patchSettings(patch: Partial<Record<SettingKey, unknown>>): Promise<void> {
  const next = { ...values } as Record<string, SettingValue>;
  for (const [key, raw] of Object.entries(patch)) {
    if (!isSettingKey(key)) continue;
    const value = normalizeSetting(SETTINGS[key], raw);
    if (value !== undefined) next[key] = value;
  }
  const fixed = crossFix(next as SettingValues);
  const changed: Partial<Record<SettingKey, unknown>> = {};
  for (const key of SETTING_KEYS) if (fixed[key] !== values[key]) changed[key] = fixed[key];
  const keys = Object.keys(changed) as SettingKey[];
  if (!keys.length) return queue;
  for (const key of keys) pending.set(key, changed[key]);
  commit(fixed);

  const settle = () => {
    for (const key of keys) if (pending.get(key) === changed[key]) pending.delete(key);
  };
  const run = async () => {
    try {
      const saved = await saver(changed);
      settle();
      if (saved) receiveSettings(saved);
    } catch (error) {
      settle();
      saveErrorListener(String(error));
    }
  };
  queue = queue.then(run);
  return queue;
}

export function setSetting<K extends SettingKey>(key: K, raw: unknown): Promise<void> {
  return patchSettings({ [key]: raw } as Partial<Record<SettingKey, unknown>>);
}

/** 「Reset all」: one save with every key that is not at its default. */
export function resetSettings(): Promise<void> {
  return patchSettings(defaultSettings());
}
