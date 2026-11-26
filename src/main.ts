/**
 * Main window. The frame comes from unity-slot-widget (src/shell): title bar strips, a side panel
 * with square tabs (projects, runs, styles), the prefab grid in the free width, the preview and the
 * make controls on the right, a banner for what needs a person now, and a status bar. Notices show
 * in the status bar for a moment (flashStatus) instead of toasts.
 *
 * Projects: added with the folder picker (Ctrl+O), by dropping a folder on the window, by typing a
 * path, or from Unity Hub's list. Picking one reads its prefabs without Unity (uis-core
 * prefabs.rs, cached) and checks how a capture would run (lock file, version, packages). The state
 * is checked again every 30 seconds while the window has the focus.
 *
 * Making icons: 「Make icon」 starts a whole run (uis-core pipeline.rs through run_start): the
 * capture of the picked prefabs that have no picture yet (one request), then for each in turn the
 * prompt, the rewrite of the note, the picture, the finish and the checks. The step row follows
 * the run's events; Esc asks whether to stop it. Results land in the 「Runs」 tab too.
 *
 * The screen state (side panel, widths, project, filters, preview background) comes back on the
 * next start (localStorage, shell/prefs). Settings live in settings.json in the tool folder (Rust).
 */

import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  call,
  captureImage,
  captureOnly,
  copyCodex,
  exportIcons,
  exportPick,
  inTauri,
  jobGet,
  jobsList,
  postRerun,
  prefabThumbs,
  promptPreview,
  promptRewrite,
  readiness,
  runCancel,
  runStart,
} from "./backend";
import { captureErrorText } from "./capture-text";
import { applyDomTexts, currentLang, resolveLang, setLang, t } from "./i18n";
import { applyFilter, CLASS_CHIPS, DEFAULT_FILTER, folderChoices, KIND_CHIPS, nextSelection, type ClassChip, type PrefabFilter } from "./prefab-filter";
import { failureText } from "./prompt-text";
import { applyEvent, finishRun, newRun, type RunView } from "./run-state";
import { tickSteppers } from "./components/stepper";
import { renderDetails, runErrorText, type CandView, type CaptureView, type GenView, type ResultView, type RetryKind } from "./screens/details";
import { renderPacks } from "./screens/packs";
import { gridColumns, itemKey, renderChips, renderGrid } from "./screens/prefabs";
import { modeText, reasonText, renderProjects } from "./screens/projects";
import { renderRuns } from "./screens/runs";
import { aboutPanel } from "./settings-panels";
import { getSetting, onSettingsChange, onSettingsSaveError, receiveSettings, setSetting, settingsDocument, SETTING_KEYS, valueAt, whenSaved, type SettingKey } from "./settings";
import { relabelBanner, setBannerFocusHome, showBanner } from "./shell/banner";
import { showContextMenu, showMenuBelow, type MenuEntry } from "./shell/context-menu";
import { isDialogOpen, showChoice, showDialog, showPrompt } from "./shell/dialog";
import { initFrame } from "./shell/frame";
import { initKeys } from "./shell/keys";
import { initNav } from "./shell/nav";
import { isBoolean, isString, readPref, writePref } from "./shell/prefs";
import { initBodyResize } from "./shell/resize";
import { initSettingsDialog } from "./shell/settings-dialog";
import { fitStatusbar, flashStatus, setStatusPath } from "./shell/statusbar";
import { initTheme, isThemeChoice, oppositeChoice } from "./shell/theme";
import { initTitlebar } from "./shell/titlebar";
import type {
  AppInfo,
  Assembled,
  BatchFinish,
  CaptureEvent,
  HubProject,
  JobDetail,
  JobSummary,
  Kind,
  PackSummary,
  PostJobReport,
  PrefabItem,
  ProjectSummary,
  Readiness,
  RecoveryEntry,
  RunItem,
  ScanResult,
} from "./types";

function $(id: string): HTMLElement {
  const node = document.getElementById(id);
  if (!node) throw new Error(`#${id} missing`);
  return node;
}

const app = $("app");
const overlayQuery = matchMedia("(max-width: 999px)");

const nav = initNav(overlayQuery);
const titlebar = initTitlebar(inTauri);
initFrame(inTauri);
const theme = initTheme();
const settingsDialog = initSettingsDialog();

const PROBE_MS = 30_000;
const BGS = ["checker", "white", "black"];
const isStringOrNull = (v: unknown): v is string | null => v === null || typeof v === "string";
const isFilter = (v: unknown): v is PrefabFilter =>
  typeof v === "object" &&
  v !== null &&
  typeof (v as PrefabFilter).text === "string" &&
  Array.isArray((v as PrefabFilter).classes) &&
  (v as PrefabFilter).classes.every((c) => (CLASS_CHIPS as readonly string[]).includes(c)) &&
  Array.isArray((v as PrefabFilter).kinds) &&
  (v as PrefabFilter).kinds.every((k) => KIND_CHIPS.includes(k)) &&
  typeof (v as PrefabFilter).folder === "string";

let appInfo: AppInfo | null = null;
let bodyOpen = readPref("bodyOpen", true, isBoolean);
let projects: ProjectSummary[] = [];
let packs: PackSummary[] = [];
let currentId: string | null = readPref("project", null, isStringOrNull);
let scan: ScanResult | null = null;
let scanning = false;
let scanError: string | null = null;
let filter: PrefabFilter = { ...readPref("filter", DEFAULT_FILTER, isFilter), text: "" };
let selected = new Set<string>();
let anchor: string | null = null;
let focusKey: string | null = null;
let shown: PrefabItem[] = [];
let dropHover = false;
let thumbs: Record<string, string> = {};
let previewBg = readPref("previewBg", "checker", (v): v is string => isString(v) && BGS.includes(v));
const emptyCapture = (): CaptureView => ({ running: false, progress: "", error: null, images: {}, failures: {}, outputs: {} });
/** Capture state of the open project (cleared when another project is opened). */
let cap: CaptureView = emptyCapture();
const settingNumber = (key: string, d: number) => {
  const v = valueAt(settingsDocument(), key);
  return typeof v === "number" ? v : d;
};
const settingText = (key: string, d: string) => {
  const v = valueAt(settingsDocument(), key);
  return typeof v === "string" ? v : d;
};
const exportSizes = (): number[] => {
  const v = valueAt(settingsDocument(), "exportSizes");
  return Array.isArray(v) && v.every((n) => typeof n === "number") && v.length ? (v as number[]) : [256];
};
const emptyGen = (cli: string): GenView => ({
  note: "",
  cli,
  busy: "",
  diff: null,
  error: null,
  errorStage: null,
  failure: null,
  candidates: settingNumber("generate.candidates", 1),
  outline: settingText("generate.outline", "code"),
  background: settingText("generate.background", "transparent") === "flat" ? "key" : "auto",
  result: null,
  exportName: "",
  exportSizes: exportSizes(),
});
/** Note, rewrite preview, options and result of the picked prefab(s) (cleared when the pick changes). */
let gen: GenView = emptyGen("claude");
let genFor: string | null = null;
/** The run in progress or the last one (one at a time). */
let run: RunView | null = null;
let ready: Readiness | null = null;
let imagesToday = 0;
// 「Runs」 tab
let jobs: JobSummary[] = [];
let jobIcons: Record<string, string> = {};
let jobsLoading = false;
let opened: ResultView | null = null;
let openedDir: string | null = null;
let newRuns = 0;
// 「Styles」 tab
let packShown = "default";
let packKind = "building";
let packPrompt: Assembled | null = null;
let tick: number | null = null;

// ---------------------------------------------------------------- state helpers

function current(): ProjectSummary | null {
  return projects.find((p) => p.id === currentId) ?? null;
}

function packOf(p: ProjectSummary | null): PackSummary | null {
  return p ? (packs.find((k) => k.id === p.packId) ?? packs[0] ?? null) : null;
}

function errorText(code: string): string {
  if (code.startsWith("notProject:")) {
    const why = code.slice("notProject:".length);
    return t(`error.notProject.${["assets", "version", "manifest", "missing"].includes(why) ? why : "missing"}`);
  }
  if (code === "busy") return t("error.busy");
  if (code.startsWith("pack:")) return t("error.pack", { text: code.slice(5) });
  return t("error.other", { text: code });
}

function picked(): PrefabItem[] {
  if (!scan) return [];
  return scan.prefabs.filter((p) => selected.has(itemKey(p)));
}

const running = () => !!run?.running;
const busyNow = () => running() || cap.running || !!gen.busy;

// ---------------------------------------------------------------- drawing

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className = "", text = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
}

function renderNav() {
  renderProjects($("projects-body"), { projects, current: currentId }, { pick: pickProject, menu: projectMenu });
  renderRuns($("runs-body"), { jobs, icons: jobIcons, opened: openedDir, loading: jobsLoading }, { open: (dir) => void openJob(dir), refresh: () => void loadRuns() });
  renderPacks(
    $("packs-body"),
    { packs, projectPack: current()?.packId ?? null, shown: packShown, kind: packKind, prompt: packPrompt, lang: currentLang() },
    {
      show: (id) => {
        packShown = id;
        void loadPackPrompt();
      },
      use: (id) => void usePack(id),
      kind: (k) => {
        packKind = k;
        void loadPackPrompt();
      },
      openFolder: (dir) => void call("open_path", { path: dir }).catch((e) => flashStatus(t("status.openFailed", { text: String(e) }))),
    },
  );
  nav.setBadge("runs", newRuns, "info");
}

function renderHead() {
  const p = current();
  const title = $("project-title");
  if (!p) {
    title.textContent = t("app.title");
    return;
  }
  title.replaceChildren();
  title.append(el("span", "", p.name));
  if (p.branch) title.append(el("span", "sep", "·"), el("span", "muted", p.branch));
  const mode = el("span", `head-mode mode-${p.mode}`);
  mode.append(el("span", `state-dot mode-${p.mode}`), document.createTextNode(modeText(p)));
  mode.title = reasonText(p);
  title.append(el("span", "sep", "·"), mode);
}

function renderList() {
  const p = current();
  const tools = $("list-tools");
  const inner = $("list-inner");
  if (!p) {
    tools.hidden = true;
    const box = el("div", "empty-state");
    box.classList.toggle("drop-on", dropHover);
    box.append(el("h2", "", t("empty.title")));
    const add = el("button", "act-btn", t("projects.add"));
    add.type = "button";
    add.id = "empty-add";
    add.addEventListener("click", (e) => void addMenu(e.currentTarget as HTMLElement));
    box.append(add, el("p", "muted", inTauri ? t("empty.drop") : t("empty.preview")));
    inner.replaceChildren(box);
    return;
  }
  tools.hidden = false;
  if (!scan) {
    const box = el("div", "empty-state");
    if (scanError) box.append(el("p", "hot-text", errorText(scanError)));
    else {
      box.append(el("p", "muted", t("prefabs.scanning")));
      const ghosts = el("div", "prefab-grid ghost");
      for (let i = 0; i < 12; i++) ghosts.append(el("div", "prefab-cell ghost-cell"));
      box.append(ghosts);
    }
    inner.replaceChildren(box);
    $("list-count").textContent = "";
    return;
  }
  shown = applyFilter(scan.prefabs, filter);
  const view = { items: shown, all: scan.prefabs, filter, selected, focus: focusKey, thumbs };
  renderChips($("class-chips"), $("kind-chips"), view);
  renderGrid(inner, view);
  $("list-count").textContent = t("prefabs.count", { shown: shown.length, total: scan.prefabs.length });
  const label = $("folder-btn").querySelector<HTMLElement>(".pick-label")!;
  label.textContent = filter.folder ? filter.folder.split("/").slice(1).join("/") : t("prefabs.allFolders");
  label.parentElement!.title = filter.folder;
  $("folder-btn").classList.toggle("on", !!filter.folder);
  const find = $("find-input") as HTMLInputElement;
  if (find.value !== filter.text) find.value = filter.text;
}

function renderBody() {
  const now = picked();
  const key = now.length ? now.map((i) => i.path).join("|") : null;
  if (key !== genFor && !busyNow()) {
    const keepNote = now.length > 1 && gen.note;
    gen = { ...emptyGen(gen.cli), note: keepNote ? gen.note : "" };
    genFor = key;
    if (!run?.running) run = null;
  }
  if (now.length && opened && !busyNow()) {
    opened = null;
    openedDir = null;
  }
  const title = renderDetails(
    $("body-content"),
    {
      project: current(),
      pack: packOf(current()),
      picked: now,
      lang: currentLang(),
      capture: cap,
      ready,
      gen,
      run,
      now: Date.now(),
      bg: previewBg,
      exportDir: (getSetting("exportDir") as string) || "",
      exportRule: (getSetting("exportName") as string) || "icon_{prefab}",
      opened,
    },
    {
      setKind: (item, kind) => void setKind(item, kind),
      openFolder: (folder) => void openInProject(folder),
      copy: (text) => void copyText(text),
      capture: (items) => void runCapture(items),
      note: (text) => {
        gen.note = text;
      },
      rewriter: (cli) => {
        gen.cli = cli;
      },
      rewrite: (item) => void runRewrite(item),
      showPrompt: (item) => void showFinalPrompt(item),
      make: (items) => void startMake(items),
      retry: (kind, items) => void retry(kind, items),
      cancel: () => void askCancel(),
      option: (k, value) => {
        if (k === "candidates") gen.candidates = Number(value) || 1;
        else gen[k] = value;
      },
      pickCand: (n) => {
        const r = shownResult();
        if (!r) return;
        r.cand = n;
        r.exported = null;
        renderBody();
      },
      toggleRaw: () => {
        const r = shownResult();
        if (!r) return;
        r.showRaw = !r.showRaw;
        renderBody();
      },
      setBg: (bg) => setPreviewBg(bg),
      repost: () => void runPost(),
      exportIcon: () => void runExport(),
      exportPassed: () => void exportPassed(),
      pickExportFolder: () => void pickExportFolder(),
      exportName: (text) => {
        gen.exportName = text;
      },
      exportSize: (size, on) => {
        gen.exportSizes = on ? [...new Set([...gen.exportSizes, size])].sort((a, b) => a - b) : gen.exportSizes.filter((s) => s !== size);
        void call("settings_set", { patch: { exportSizes: gen.exportSizes } }).then(receiveSettings).catch(() => {});
        renderBody();
      },
      openPath: (path) => void call("open_path", { path }).catch((e) => flashStatus(t("status.openFailed", { text: String(e) }))),
      checkReady: () => void checkReady(true),
      closeOpened: () => {
        opened = null;
        openedDir = null;
        render();
      },
      rerunOpened: () => void rerunOpened(),
    },
  );
  $("body-title").textContent = title || t("body.title");
}

function readyText(): { text: string; title: string; bad: boolean } {
  if (!ready) return { text: "", title: "", bad: false };
  const mark = (ok: boolean | null) => (ok ? "✓" : ok === null ? "–" : "!");
  const codexOk = ready.codex.found && ready.codex.loggedIn !== false && ready.codex.imageTool !== false;
  const claudeOk = !ready.claude.enabled ? null : ready.claude.found && ready.claude.loggedIn !== false;
  const unityOk = ready.unityCli.found ? true : null;
  const text = `Codex ${mark(codexOk)} · Claude ${mark(claudeOk)} · Unity ${mark(unityOk)}`;
  return { text, title: t("status.readyTitle"), bad: !codexOk };
}

function renderStatus() {
  const p = current();
  setStatusPath(p ? p.path : (appInfo?.home ?? ""));
  $("status-mode").textContent = p ? modeText(p) : "";
  $("status-mode").title = p ? reasonText(p) : "";
  const r = readyText();
  $("status-ready").textContent = r.text;
  $("status-ready").title = r.title;
  $("status-ready").classList.toggle("warn-text", r.bad);
  $("status-busy").hidden = !scanning && !cap.running && !gen.busy && !running();
  let progress = "";
  if (running() && run) {
    const it = run.items.find((i) => i.state === "running");
    const s = Math.round((Date.now() - run.startedMs) / 1000);
    progress = it ? t("status.making", { name: it.name, s }) : t("status.capturing", { s });
  } else if (cap.running) progress = cap.progress;
  else if (scanning) progress = t("status.scanning");
  else if (scan) progress = t("status.scanned", { n: scan.total, ms: scan.elapsedMs });
  $("status-progress-text").textContent = progress;
  $("status-today").textContent = t("status.today", { n: imagesToday });
  $("status-version").textContent = appInfo ? t("status.version", { version: appInfo.version }) : "";
  fitStatusbar();
}

function render() {
  renderNav();
  renderHead();
  renderList();
  renderBody();
  renderStatus();
}

/** Only the grid and what depends on the selection (a press in the grid). */
function renderSelection() {
  const inner = $("list-inner");
  for (const cell of inner.querySelectorAll<HTMLElement>(".prefab-cell")) {
    const key = cell.dataset.key!;
    const on = selected.has(key);
    cell.classList.toggle("selected", on);
    cell.setAttribute("aria-selected", String(on));
    cell.classList.toggle("focus", key === focusKey);
  }
  renderBody();
}

function setPreviewBg(bg: string) {
  previewBg = bg;
  writePref("previewBg", bg);
  renderBody();
}

// ---------------------------------------------------------------- projects

async function loadProjects() {
  projects = (await call<ProjectSummary[]>("project_list").catch(() => null)) ?? [];
  packs = (await call<PackSummary[]>("pack_list").catch(() => null)) ?? [];
  if (currentId && !projects.some((p) => p.id === currentId)) currentId = null;
  if (!currentId && projects.length) currentId = projects[0].id;
  if (!packs.some((p) => p.id === packShown)) packShown = current()?.packId ?? packs[0]?.id ?? "default";
}

function pickProject(id: string) {
  if (id === currentId && scan) return;
  currentId = id;
  writePref("project", id);
  if (!cap.running) cap = emptyCapture();
  scan = null;
  scanError = null;
  thumbs = {};
  selected = new Set();
  anchor = null;
  focusKey = null;
  render();
  void runScan(false);
}

async function runScan(force: boolean) {
  const id = currentId;
  if (!id) return;
  scanning = true;
  renderStatus();
  try {
    const result = await call<ScanResult>("prefab_scan", { id, force, includeModels: false });
    if (id !== currentId) return;
    scan = result;
    scanError = null;
    // keep what is still there
    const keys = new Set(result.prefabs.map(itemKey));
    selected = new Set([...selected].filter((k) => keys.has(k)));
    if (focusKey && !keys.has(focusKey)) focusKey = null;
    void loadThumbs();
  } catch (e) {
    if (id !== currentId) return;
    scanError = String(e);
    if (scanError === "busy") flashStatus(errorText(scanError));
  } finally {
    scanning = false;
  }
  applyPreviewPick();
  render();
}

async function loadThumbs() {
  const id = currentId;
  if (!id) return;
  const got = await prefabThumbs(id).catch(() => null);
  if (!got || id !== currentId) return;
  thumbs = got;
  renderList();
}

async function addPath(path: string) {
  try {
    const p = await call<ProjectSummary>("project_add", { path });
    await loadProjects();
    nav.show("projects");
    pickProject(p.id);
  } catch (e) {
    flashStatus(errorText(String(e)), 5000);
  }
}

async function pickFolder() {
  if (!inTauri) {
    flashStatus(t("empty.preview"));
    return;
  }
  try {
    const p = await call<ProjectSummary | null>("project_pick");
    if (!p) return;
    await loadProjects();
    nav.show("projects");
    pickProject(p.id);
  } catch (e) {
    flashStatus(errorText(String(e)), 5000);
  }
}

/** A path typed or pasted (the same checks as a picked folder). */
async function typePath() {
  const text = await showPrompt({
    title: t("projects.typePath"),
    message: t("projects.typePathHint"),
    value: "",
    placeholder: "C:\\Projects\\MyGame",
    confirmLabel: t("projects.add"),
    cancelLabel: t("dialog.cancel"),
  });
  const path = text?.trim().replace(/^"(.*)"$/, "$1");
  if (path) await addPath(path);
}

async function hubMenu(): Promise<MenuEntry[]> {
  const hub = (await call<HubProject[]>("hub_projects").catch(() => null)) ?? [];
  const known = new Set(projects.map((p) => p.path.toLowerCase()));
  const usable = hub.filter((h) => h.exists && !known.has(h.path.toLowerCase())).slice(0, 12);
  if (!usable.length) return [{ label: t("projects.hubNone"), action: () => {}, disabled: true }];
  return usable.map((h) => ({ label: `${h.title}${h.version ? `  (${h.version})` : ""}`, id: "hub-project", action: () => void addPath(h.path) }));
}

async function addMenu(button: HTMLElement) {
  const entries: MenuEntry[] = [
    { label: t("projects.pickFolder"), shortcut: "Ctrl+O", id: "pick-folder", action: () => void pickFolder() },
    { label: t("projects.typePath"), id: "type-path", action: () => void typePath() },
    "separator",
    { label: t("projects.fromHub"), action: () => {}, disabled: true },
    ...(await hubMenu()),
  ];
  showMenuBelow(button, entries);
}

function projectMenu(id: string, event: MouseEvent) {
  const p = projects.find((x) => x.id === id);
  if (!p) return;
  showContextMenu(event, [
    { label: t("projects.openFolder"), action: () => void openInProject(p.path) },
    { label: t("projects.recheck"), action: () => void refreshProjects(true) },
    "separator",
    { label: t("projects.remove"), danger: true, action: () => void removeProject(id) },
  ]);
}

async function removeProject(id: string) {
  await call("project_remove", { id }).catch((e) => flashStatus(errorText(String(e))));
  if (id === currentId) {
    currentId = null;
    scan = null;
    selected = new Set();
  }
  await loadProjects();
  if (currentId && !scan) void runScan(false);
  render();
}

/** Checks every project again (lock file, branch); `rescan` reads the prefabs too. */
async function refreshProjects(rescan: boolean) {
  await loadProjects();
  render();
  if (rescan) void runScan(false);
}

// ---------------------------------------------------------------- grid

function setFilter(next: Partial<PrefabFilter>) {
  filter = { ...filter, ...next };
  writePref("filter", { ...filter, text: "" });
  renderList();
}

function toggleChip(group: string, value: string) {
  if (group === "class") {
    const c = value as ClassChip;
    const classes = filter.classes.includes(c) ? filter.classes.filter((x) => x !== c) : [...filter.classes, c];
    setFilter({ classes: classes.length ? classes : [c] });
  } else {
    const k = value as Kind;
    setFilter({ kinds: filter.kinds.includes(k) ? filter.kinds.filter((x) => x !== k) : [...filter.kinds, k] });
  }
}

function folderMenu(button: HTMLElement) {
  if (!scan) return;
  const choices = folderChoices(applyFilter(scan.prefabs, { ...filter, folder: "" }));
  const entries: MenuEntry[] = [
    { label: t("prefabs.allFolders"), checked: !filter.folder, action: () => setFilter({ folder: "" }) },
    "separator",
    ...choices.map((c) => ({
      label: `${c.folder.split("/").slice(1).join("/")}  (${c.count})`,
      checked: filter.folder === c.folder,
      action: () => setFilter({ folder: c.folder }),
    })),
  ];
  showMenuBelow(button, entries);
}

function clickCell(e: MouseEvent) {
  const cell = (e.target as Element).closest<HTMLElement>(".prefab-cell");
  if (!cell || !cell.dataset.key) return;
  const order = shown.map(itemKey);
  const next = nextSelection(order, selected, anchor, cell.dataset.key, { ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey });
  selected = next.selected;
  anchor = next.anchor;
  focusKey = cell.dataset.key;
  renderSelection();
}

function moveFocus(delta: number | "home" | "end", extend: boolean) {
  if (!shown.length) return;
  const order = shown.map(itemKey);
  let i = focusKey ? order.indexOf(focusKey) : -1;
  if (delta === "home") i = 0;
  else if (delta === "end") i = order.length - 1;
  else i = i < 0 ? 0 : Math.min(order.length - 1, Math.max(0, i + delta));
  const key = order[i];
  const next = extend ? nextSelection(order, selected, anchor ?? key, key, { ctrl: false, shift: true }) : nextSelection(order, selected, anchor, key, { ctrl: false, shift: false });
  selected = next.selected;
  anchor = next.anchor;
  focusKey = key;
  renderSelection();
  $("list-inner").querySelector<HTMLElement>(`.prefab-cell[data-key="${CSS.escape(key)}"]`)?.scrollIntoView({ block: "nearest" });
}

function gridKeysActive(): boolean {
  return document.activeElement === $("list") && !!scan;
}

function cellMenu(e: MouseEvent) {
  const cell = (e.target as Element).closest<HTMLElement>(".prefab-cell");
  const item = cell && scan?.prefabs.find((p) => itemKey(p) === cell.dataset.key);
  if (!item) return;
  e.preventDefault();
  if (!selected.has(itemKey(item))) {
    selected = new Set([itemKey(item)]);
    anchor = focusKey = itemKey(item);
    renderSelection();
  }
  showContextMenu(e, [
    ...KIND_CHIPS.map((k) => ({ label: t("prefabs.setKind", { kind: t(`kind.${k}`) }), checked: item.kind === k, action: () => void setKind(item, k) })),
    { label: t("details.kindAuto"), action: () => void setKind(item, null) },
    "separator",
    { label: t("details.openFolder"), action: () => void openInProject(item.folder) },
  ]);
}

async function setKind(item: PrefabItem, kind: string | null) {
  if (!currentId || !item.guid) {
    flashStatus(t("details.noGuid"));
    return;
  }
  try {
    await call("prefab_override", { id: currentId, guid: item.guid, patch: { kind } });
    await runScan(false);
  } catch (e) {
    flashStatus(errorText(String(e)));
  }
}

/** Thumbnails for the visible 3D prefabs that have none (one capture request, after a yes). */
async function makeThumbs() {
  const id = currentId;
  const p = current();
  if (!id || !p || !scan || busyNow()) return;
  const max = settingNumber("thumbnails.maxPerRun", 200);
  const items = shown.filter((i) => (i.class === "3d" || i.class === "model") && !(i.guid && thumbs[i.guid])).slice(0, max);
  if (!items.length) {
    flashStatus(t("thumbs.none"));
    return;
  }
  const secs = p.mode === "attach" ? 2 + items.length * 0.25 : 25 + items.length * 0.3;
  const ok = await showDialog({
    title: t("thumbs.title"),
    message: t("thumbs.confirm", { n: items.length, s: Math.round(secs), mode: modeText(p) }),
    confirmLabel: t("thumbs.make"),
    cancelLabel: t("dialog.cancel"),
  });
  if (!ok) return;
  cap.running = true;
  cap.progress = t("capture.running");
  renderStatus();
  try {
    const report = await captureOnly(id, items.map((i) => i.path), (e) => {
      cap.progress = progressText(e);
      renderStatus();
    }, "thumb");
    if (report.error) flashStatus(captureErrorText(report.error), 6000);
    else flashStatus(t("thumbs.done", { n: report.items.filter((i) => i.ok).length }), 5000);
  } catch (e) {
    flashStatus(errorText(String(e)), 5000);
  } finally {
    cap.running = false;
    cap.progress = "";
    renderStatus();
    void loadThumbs();
  }
}

// ---------------------------------------------------------------- capture

function progressText(e: CaptureEvent): string {
  return t("capture.progress", { stage: t(`capture.stage.${e.stage}`), done: e.done, total: e.total });
}

/** Captures the items (in the open editor or in batch mode) and shows what came out. */
async function runCapture(items: PrefabItem[]) {
  const id = currentId;
  if (!id || busyNow() || !items.length) return;
  cap.running = true;
  cap.error = null;
  cap.progress = t("capture.running");
  renderBody();
  renderStatus();
  try {
    const report = await captureOnly(
      id,
      items.map((i) => i.path),
      (e) => {
        cap.progress = progressText(e);
        renderStatus();
        if (selected.size) renderBody();
      },
    );
    cap.error = report.error;
    for (const it of report.items) {
      if (it.ok && it.output) {
        delete cap.failures[it.prefab];
        cap.outputs[it.prefab] = it.output;
        cap.images[it.prefab] = await captureImage(it.output).catch(() => "");
      } else if (it.error) {
        cap.failures[it.prefab] = it.error;
      }
    }
    const ok = report.items.filter((i) => i.ok).length;
    const s = (report.elapsedMs / 1000).toFixed(1);
    if (report.error) flashStatus(captureErrorText(report.error), 6000);
    else flashStatus(report.failed ? t("capture.doneFailed", { ok, failed: report.failed, s }) : t("capture.done", { ok, s }), 5000);
    if (report.pending) void checkRecovery();
  } catch (e) {
    cap.error = String(e) === "busy" ? "busy" : String(e);
    flashStatus(errorText(String(e)), 5000);
  } finally {
    cap.running = false;
    cap.progress = "";
    if (currentId === id) {
      renderBody();
      renderStatus();
      void refreshProjects(false);
      void loadThumbs();
    } else {
      cap = emptyCapture();
    }
  }
}

// ---------------------------------------------------------------- prompt

const cliName = (cli: string) => (cli === "claude" ? "Claude" : "Codex");

/** The note worked into the fixed prompt (no picture): the before/after shows under the note. */
async function runRewrite(item: PrefabItem) {
  const p = current();
  if (!p || busyNow() || !gen.note.trim()) return;
  gen.busy = "rewrite";
  gen.error = null;
  gen.failure = null;
  renderBody();
  renderStatus();
  try {
    gen.diff = await promptRewrite(p.packId, item.kind, gen.note, gen.cli);
    if (gen.diff.failure) {
      gen.error = `rewrite:${gen.diff.failure.kind}`;
      gen.errorStage = "rewrite";
      gen.failure = gen.diff.failure;
      flashStatus(failureText(gen.diff.failure), 6000);
    } else if (gen.diff.fallbackFrom) flashStatus(t("prompt.fallback", { from: cliName(gen.diff.fallbackFrom) }), 6000);
  } catch (e) {
    gen.error = String(e);
    gen.errorStage = "rewrite";
  } finally {
    gen.busy = "";
    renderBody();
    renderStatus();
  }
}

/** The whole prompt that would go now (the rewrite's when there is one, else the fixed one). */
async function showFinalPrompt(item: PrefabItem) {
  const p = current();
  if (!p) return;
  let text = gen.diff?.ok && gen.diff.finalText && gen.diff.note === gen.note ? gen.diff.finalText : "";
  if (!text) text = (await promptPreview(p.packId, item.kind).catch(() => null))?.text ?? "";
  await showDialog({ title: t("make.showPrompt"), message: gen.note.trim() && !gen.diff ? t("make.promptFixedOnly") : "", detail: text });
}

// ---------------------------------------------------------------- make: the whole run

/** Asks about a note that turns the outline off (the before/after said so). True: go on. */
async function outlineChoice(): Promise<boolean | null> {
  if (!gen.diff || gen.diff.note !== gen.note || gen.diff.diff.outline !== "off") return false;
  const pick = await showChoice({
    title: t("outlineOff.title"),
    message: t("outlineOff.message"),
    choices: [
      { value: "keep", label: t("outlineOff.keep") },
      { value: "off", label: t("outlineOff.off"), kind: "primary" },
    ],
    cancelLabel: t("dialog.cancel"),
  });
  if (pick === null) return null;
  return pick === "off";
}

async function startMake(items: PrefabItem[], extra: { note?: string; cli?: string; noRefs?: boolean } = {}) {
  const p = current();
  const id = currentId;
  if (!p || !id || busyNow() || !items.length) return;
  const offOk = extra.note === undefined ? await outlineChoice() : false;
  if (offOk === null) return;
  const note = extra.note ?? gen.note;
  const cli = extra.cli ?? gen.cli;
  gen.error = null;
  gen.errorStage = null;
  gen.failure = null;
  gen.result = null;
  run = newRun(
    items.map((i) => ({ asset: i.path, name: i.name, captured: !!cap.outputs[i.path] })),
    Date.now(),
  );
  startTick();
  renderBody();
  renderStatus();
  try {
    const report = await runStart(
      {
        id,
        items: items.map((i) => ({ asset: i.path, kind: i.kind, ...(cap.outputs[i.path] ? { capture: cap.outputs[i.path] } : {}) })),
        note,
        cli,
        candidates: gen.candidates,
        outline: gen.outline,
        background: gen.background,
        outlineOffOk: offOk,
        noRefs: !!extra.noRefs,
      },
      (e) => {
        if (!run) return;
        applyEvent(run, e, Date.now());
        if (e.state === "progress") return;
        renderBody();
        renderStatus();
      },
    );
    if (id !== currentId || !run) return;
    finishRun(run, report, Date.now());
    for (const it of report.items) {
      if (it.capture && !cap.outputs[it.asset]) {
        cap.outputs[it.asset] = it.capture;
        cap.images[it.asset] = await captureImage(it.capture).catch(() => "");
      }
      if (it.stage === "capture" && it.error) cap.failures[it.asset] = it.error;
    }
    if (report.items.length === 1) await settleOne(report.items[0]);
    else if (report.stopped) flashStatus(runErrorText(`image:${report.stopped}`), 8000);
    else {
      const c = report.items.filter((i) => i.state === "ok").length;
      flashStatus(t("run.done", { ok: c, n: report.items.length, s: Math.round(report.elapsedMs / 1000) }), 6000);
    }
    if (report.capture?.pending) void checkRecovery();
    if (report.items.some((i) => i.jobDir)) noteNewRuns(report.items.filter((i) => i.jobDir).length);
  } catch (e) {
    if (run) {
      run.running = false;
      run.error = String(e);
    }
    gen.error = String(e);
    gen.errorStage = null;
    flashStatus(errorText(String(e)), 6000);
  } finally {
    stopTick();
    imagesToday = await call<number>("images_today").catch(() => imagesToday);
    renderBody();
    renderStatus();
    void loadThumbs();
  }
}

/** The result panel (or the failure and its buttons) for a one-item run. */
async function settleOne(it: RunItem) {
  if (it.rewrite && it.rewrite.cli) gen.diff = it.rewrite;
  if (it.state !== "ok") {
    gen.error = it.error ?? it.state;
    gen.errorStage = it.stage;
    gen.failure = it.rewrite?.failure ?? it.candidates.find((c) => c.failure)?.failure ?? null;
    if (it.state === "cancelled") {
      gen.error = null;
      flashStatus(t("run.cancelled"), 5000);
    } else flashStatus(gen.failure ? failureText(gen.failure) : runErrorText(gen.error), 8000);
    if (!it.candidates.some((c) => c.post)) return;
  }
  gen.result = await resultFrom(it.jobDir ?? "", it.name, it.kind, it.candidates.map(candFrom), it.best, it.outlineKept, null);
  if (it.outlineKept) flashStatus(t("result.outlineKept"), 8000);
  else if (it.state === "ok") flashStatus(t("make.done", { s: Math.round(it.elapsedMs / 1000) }), 5000);
}

function candFrom(c: RunItem["candidates"][number]): CandView {
  return { cand: c.cand, post: c.post, failure: c.failure, passed: c.passed, checked: c.checked };
}

function scoreOf(p: PostJobReport): { passed: number; checked: number } {
  let passed = 0;
  let checked = 0;
  for (const s of p.sizes) {
    passed += s.checks.items.filter((i) => i.pass).length;
    checked += s.checks.items.length;
  }
  return { passed, checked };
}

async function resultFrom(job: string, name: string, kind: string, cands: CandView[], best: number | null, outlineKept: boolean, from: ResultView["from"]): Promise<ResultView> {
  const r: ResultView = { job, name, kind, cands, best, cand: best ?? cands[0]?.cand ?? 1, icons: {}, raws: {}, showRaw: false, exported: null, outlineKept, from };
  for (const c of cands) {
    if (c.post) {
      r.icons[c.cand] = await captureImage(c.post.sizes[0].path).catch(() => "");
      if (c.post.raw) r.raws[c.cand] = await captureImage(c.post.raw).catch(() => "");
    }
  }
  return r;
}

/** The result on the right now: an opened job, else the picked prefab's. */
function shownResult(): ResultView | null {
  return opened ?? gen.result;
}

async function retry(kind: RetryKind, items: PrefabItem[]) {
  if (kind === "otherCli") {
    gen.cli = gen.cli === "claude" ? "codex" : "claude";
    await startMake(items, { cli: gen.cli });
  } else if (kind === "withoutNote") await startMake(items, { note: "" });
  else if (kind === "noRefs") await startMake(items, { noRefs: true });
  else await startMake(items, { note: gen.note });
}

/** Esc or 「Stop」: asks, then stops the run (a capture finishes its request first). */
async function askCancel() {
  if (!running() || !run || run.cancelling) return;
  const ok = await showDialog({ title: t("cancel.title"), message: t("cancel.message"), confirmLabel: t("cancel.stop"), cancelLabel: t("cancel.keep"), danger: true });
  if (!ok || !run?.running) return;
  run.cancelling = true;
  await runCancel().catch(() => false);
  renderBody();
}

function startTick() {
  stopTick();
  tick = window.setInterval(() => {
    tickSteppers($("body-content"), Date.now());
    renderStatus();
  }, 1000);
}

function stopTick() {
  if (tick !== null) window.clearInterval(tick);
  tick = null;
}

// ---------------------------------------------------------------- finish again, export

/** Post-processing of the shown candidate with the picked outline and background (no new picture). */
async function runPost() {
  const r = shownResult();
  if (!r || busyNow()) return;
  gen.busy = "post";
  renderBody();
  renderStatus();
  try {
    const post = await postRerun(r.job, r.cand, { outline: gen.outline, background: gen.background });
    const c = r.cands.find((x) => x.cand === r.cand);
    if (c) {
      c.post = post;
      Object.assign(c, scoreOf(post));
    }
    r.icons[r.cand] = await captureImage(post.sizes[0].path).catch(() => "");
    r.exported = null;
    r.showRaw = false;
    flashStatus(post.pass ? t("checks.pass") : t("checks.fail", { n: post.sizes[0].checks.failed.length }), 4000);
  } catch (e) {
    flashStatus(t("result.err.post", { code: String(e) }), 6000);
  } finally {
    gen.busy = "";
    renderBody();
    renderStatus();
  }
}

/** Copies the shown candidate's icons to the export folder from the settings. */
async function runExport() {
  const r = shownResult();
  const folder = (getSetting("exportDir") as string) || "";
  if (!r || busyNow()) return;
  if (!folder) {
    await pickExportFolder();
    if (!getSetting("exportDir")) return;
  }
  gen.busy = "export";
  renderBody();
  try {
    r.exported = await exportIcons(r.job, r.cand, gen.exportSizes, r.name, (getSetting("exportDir") as string) || null, gen.exportName.trim() || null);
    if (r.exported.ok) flashStatus(t("result.exported", { n: r.exported.files.length, folder: r.exported.folder }), 5000);
    if (r.exported.ok && openedDir) void loadRuns();
  } catch (e) {
    r.exported = { ok: false, folder: "", files: [], error: String(e) };
  } finally {
    gen.busy = "";
    renderBody();
  }
}

/** Several items: exports the recommended candidate of every item that passed. */
async function exportPassed() {
  if (!run?.report || busyNow()) return;
  const folder = (getSetting("exportDir") as string) || "";
  if (!folder) return;
  gen.busy = "export";
  renderBody();
  let files = 0;
  try {
    for (const it of run.report.items) {
      if (it.state !== "ok" || !it.pass || !it.jobDir || !it.best) continue;
      const x = await exportIcons(it.jobDir, it.best, gen.exportSizes, it.name, folder, null);
      if (x.ok) files += x.files.length;
      else flashStatus(t("result.err.other", { code: x.error ?? "" }), 6000);
    }
    gen.result = { job: "", name: "", kind: "", cands: [], best: null, cand: 1, icons: {}, raws: {}, showRaw: false, outlineKept: false, exported: { ok: true, folder, files: Array.from({ length: files }, () => ({ size: 0, path: "" })), error: null } };
    flashStatus(t("result.exported", { n: files, folder }), 6000);
    void loadRuns();
  } finally {
    gen.busy = "";
    renderBody();
  }
}

async function pickExportFolder() {
  if (!inTauri) {
    flashStatus(t("empty.preview"));
    return;
  }
  try {
    const dir = await exportPick();
    if (!dir) return;
    await setSetting("exportDir", dir);
    renderBody();
  } catch (e) {
    const code = String(e);
    flashStatus(code.startsWith("insideProject:") ? t("result.err.insideProject", { path: code.slice(14) }) : errorText(code), 6000);
  }
}

// ---------------------------------------------------------------- 「Runs」 tab

async function loadRuns() {
  jobsLoading = true;
  const list = await jobsList(60).catch(() => null);
  jobsLoading = false;
  jobs = list ?? [];
  for (const j of jobs) {
    if (j.icon && !jobIcons[j.dir]) jobIcons[j.dir] = await captureImage(j.icon).catch(() => "");
  }
  renderNav();
}

function noteNewRuns(n: number) {
  if (nav.current() !== "runs" || !nav.isOpen()) newRuns += n;
  void loadRuns();
}

function onTabShown() {
  if (nav.current() === "runs") {
    newRuns = 0;
    nav.setBadge("runs", 0);
    void loadRuns();
  } else if (nav.current() === "packs") void loadPackPrompt();
}

async function openJob(dir: string) {
  if (busyNow()) return;
  try {
    const d: JobDetail = await jobGet(dir);
    const job = d.job as Record<string, unknown>;
    const str = (k: string) => (typeof job[k] === "string" ? (job[k] as string) : "");
    const cands: CandView[] = d.posts.map((p) => ({ cand: p.candidate, post: p, failure: null, ...scoreOf(p) }));
    const best = typeof job.best === "number" ? job.best : (cands.find((c) => c.post?.pass)?.cand ?? cands[0]?.cand ?? null);
    const summary = jobs.find((j) => j.dir === dir);
    opened = await resultFrom(d.dir, str("name") || summary?.name || "?", str("kind") || "generic", cands, best, false, {
      at: summary?.at ?? "",
      project: str("projectName") || str("project"),
      note: str("note"),
      asset: str("asset"),
    });
    if (d.export) opened.exported = { ok: true, folder: d.export.folder, files: d.export.files, error: null };
    openedDir = dir;
    selected = new Set();
    renderSelection();
    renderNav();
    $("body-main").scrollTop = 0;
    if (overlayQuery.matches) nav.setOpen(false);
  } catch (e) {
    flashStatus(errorText(String(e)), 5000);
  }
}

/** 「Run again」 for an opened job: its project and prefab, with its note. */
async function rerunOpened() {
  const r = opened;
  if (!r?.from?.asset) return;
  const job = jobs.find((j) => j.dir === openedDir);
  const proj = projects.find((p) => job?.project && p.path.toLowerCase() === job.project.toLowerCase());
  if (!proj) {
    flashStatus(t("runs.noProject"), 5000);
    return;
  }
  const note = r.from.note;
  opened = null;
  openedDir = null;
  if (proj.id !== currentId) {
    pickProject(proj.id);
    for (let i = 0; i < 100 && (!scan || scanning); i++) await new Promise((res) => setTimeout(res, 100));
  }
  const item = scan?.prefabs.find((p) => p.path === r.from!.asset);
  if (!item) {
    flashStatus(t("runs.noPrefab"), 5000);
    render();
    return;
  }
  selected = new Set([itemKey(item)]);
  anchor = focusKey = itemKey(item);
  renderSelection();
  gen.note = note;
  renderBody();
  await startMake([item], { note });
}

// ---------------------------------------------------------------- 「Styles」 tab

async function loadPackPrompt() {
  packPrompt = await promptPreview(packShown, packKind).catch(() => null);
  renderNav();
}

async function usePack(id: string) {
  if (!currentId) return;
  try {
    await call("project_set_pack", { id: currentId, packId: id });
    await loadProjects();
    render();
    void runScan(false);
  } catch (e) {
    flashStatus(errorText(String(e)), 5000);
  }
}

// ---------------------------------------------------------------- readiness, recovery

/** Readiness of the CLIs; a banner when Codex is missing or has to be copied out of the desktop app. */
async function checkReady(refresh: boolean) {
  ready = await readiness(refresh).catch(() => null);
  imagesToday = (await call<number>("images_today").catch(() => 0)) ?? 0;
  renderBody();
  renderStatus();
  if (!ready) return;
  const badPath = !ready.codex.found && !!String(getSetting("codex.path") ?? "").trim();
  if (!badPath && ready.codexApp && (!ready.codex.found || ready.codexCopyNeeded)) {
    const mb = Math.round(ready.codexApp.bytes / 1048576);
    showBanner({
      key: `codex-copy-${ready.codexApp.version}`,
      message: ready.codex.found ? t("codex.copyNewer", { version: ready.codexApp.version }) : t("codex.copyNeeded", { mb }),
      warn: !ready.codex.found,
      actions: [{ label: t("codex.copy"), run: () => void runCopyCodex() }],
    });
  } else if (!ready.codex.found) {
    showBanner({
      key: `codex-missing-${ready.codex.problem ?? ""}`,
      message: badPath ? t("codex.badPath") : t("codex.missing"),
      warn: true,
      actions: [
        { label: t("codex.setPath"), run: () => settingsDialog.open("generate") },
        { label: t("ready.check"), run: () => void checkReady(true) },
      ],
    });
  } else if (ready.codex.loggedIn === false) {
    showBanner({ key: "codex-login", message: t("llm.err.auth", { cli: "Codex", text: "" }), warn: true, actions: [{ label: t("ready.check"), run: () => void checkReady(true) }] });
  } else showBanner(null);
}

async function runCopyCodex() {
  try {
    const info = await copyCodex((done, total) => {
      $("status-progress-text").textContent = t("codex.copying", { done: Math.round(done / 1048576), total: Math.round(total / 1048576) });
      $("status-busy").hidden = done >= total;
    });
    showBanner(null);
    flashStatus(t("codex.copied", { version: info.cliVersion ?? info.appVersion }), 6000);
  } catch (e) {
    flashStatus(t("codex.copyFailed", { text: String(e) }), 8000);
  } finally {
    $("status-busy").hidden = true;
    void checkReady(true);
  }
}

/** Batch captures left by an earlier run: a banner to clean them up (or one saying Unity still runs). */
async function checkRecovery() {
  const left = (await call<RecoveryEntry[]>("recovery_status").catch(() => null)) ?? [];
  if (!left.length) return;
  const running = left.filter((e) => e.running).length;
  if (running === left.length) {
    showBanner({ key: `recovery-running-${left.map((e) => e.runId).join(",")}`, message: t("recovery.running", { n: running }) });
    return;
  }
  showBanner({
    key: `recovery-${left.map((e) => e.runId).join(",")}`,
    message: t("recovery.pending", { n: left.length - running }),
    warn: true,
    actions: [
      {
        label: t("recovery.run"),
        run: () =>
          void call<BatchFinish[]>("recovery_run")
            .then((done) => {
              flashStatus(t("recovery.done", { n: done.length }), 5000);
              showBanner(null);
              void checkRecovery();
            })
            .catch((e) => flashStatus(errorText(String(e)), 5000)),
      },
    ],
  });
}

// ---------------------------------------------------------------- small actions

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    flashStatus(t("status.copied"));
  } catch {
    flashStatus(text);
  }
}

/** A folder from the project (`Assets/...`) or an absolute one, in Explorer. */
async function openInProject(folder: string) {
  const p = current();
  if (!p) return;
  const path = /^[A-Za-z]:[\\/]/.test(folder) ? folder : `${p.path}\\${folder.replace(/\//g, "\\")}`;
  await call("open_path", { path }).catch((e) => flashStatus(t("status.openFailed", { text: String(e) })));
}

function setBodyOpen(open: boolean) {
  bodyOpen = open;
  app.dataset.body = open ? "open" : "hidden";
  writePref("bodyOpen", open);
}

function openSettings() {
  settingsDialog.open();
}

function toggleTheme() {
  void setSetting("theme", oppositeChoice(theme.base()));
}

function applyTexts() {
  applyDomTexts();
  titlebar.relabel();
  nav.relabel();
  relabelBanner();
  settingsDialog.relabel();
  render();
}

function onSettingsChanged(changed: SettingKey[], first = false) {
  if (first || changed.includes("language")) {
    setLang(resolveLang(getSetting("language") as string));
    applyTexts();
  }
  if (first || changed.includes("theme")) {
    const choice = getSetting("theme");
    theme.apply(isThemeChoice(choice) ? choice : "system", first ? 0 : 250);
  }
  // Listeners run before the file is written: check the CLIs once the new paths are saved
  if (!first && changed.some((k) => k.startsWith("codex.") || k.startsWith("claude."))) void whenSaved().then(() => checkReady(true));
  if (!first && changed.includes("exportDir")) renderBody();
}

/** The one picked prefab (the make shortcuts act on it), or all picked. */
function pickedForKeys(): PrefabItem[] {
  const now = picked();
  return now.length ? now : [];
}

function bindEvents() {
  $("settings-btn").addEventListener("click", openSettings);
  $("project-add").addEventListener("click", (e) => void addMenu(e.currentTarget as HTMLElement));
  $("status-version").addEventListener("click", () => settingsDialog.open("about"));
  $("status-ready").addEventListener("click", () => void checkReady(true));
  $("status-path").addEventListener("click", () => {
    const p = current();
    if (p) void copyText(p.path);
  });
  for (const box of [$("class-chips"), $("kind-chips")]) {
    box.addEventListener("click", (e) => {
      const b = (e.target as Element).closest<HTMLElement>(".chip-btn");
      if (b) toggleChip(b.dataset.group!, b.dataset.value!);
    });
  }
  $("folder-btn").addEventListener("click", (e) => folderMenu(e.currentTarget as HTMLElement));
  $("thumbs-btn").addEventListener("click", () => void makeThumbs());
  $("list-more").addEventListener("click", (e) =>
    showMenuBelow(e.currentTarget as HTMLElement, [
      { label: t("prefabs.rescan"), shortcut: "F5", action: () => void runScan(false) },
      { label: t("prefabs.rescanAll"), shortcut: "Shift+F5", action: () => void runScan(true) },
      { label: t("thumbs.make"), action: () => void makeThumbs() },
      "separator",
      { label: t("prefabs.resetFilter"), id: "reset-filter", action: () => setFilter({ ...DEFAULT_FILTER }) },
    ]),
  );
  const find = $("find-input") as HTMLInputElement;
  find.addEventListener("input", () => setFilter({ text: find.value }));
  $("list").addEventListener("click", clickCell);
  $("list").addEventListener("contextmenu", cellMenu);
  $("list").addEventListener("dblclick", (e) => {
    if ((e.target as Element).closest(".prefab-cell")) $("body-main").focus();
  });
  $("nav-tabs").addEventListener("click", () => setTimeout(onTabShown, 0));
  $("nav-tabs").addEventListener("keydown", () => setTimeout(onTabShown, 0));
  setBannerFocusHome(() => $("list").focus());
  initBodyResize({ fold: () => setBodyOpen(false) });
  const inGrid = (fn: () => void) => () => gridKeysActive() && (fn(), true);
  const showTab = (tab: string) => () => {
    nav.show(tab);
    onTabShown();
  };
  initKeys([
    { keys: "Ctrl+1", run: showTab("projects") },
    { keys: "Ctrl+2", run: showTab("runs") },
    { keys: "Ctrl+3", run: showTab("packs") },
    { keys: "Ctrl+B", run: () => nav.toggle(), inFields: true },
    { keys: "Ctrl+\\", run: () => setBodyOpen(!bodyOpen), inFields: true },
    { keys: "Ctrl+O", run: () => void pickFolder(), inFields: true },
    {
      keys: "Ctrl+F",
      inFields: true,
      run: () => {
        if (!scan) return false;
        find.focus();
        find.select();
      },
    },
    { keys: "F5", run: () => void runScan(false), inFields: true },
    { keys: "Shift+F5", run: () => void runScan(true), inFields: true },
    {
      keys: "Ctrl+R",
      inFields: true,
      run: () => {
        const items = pickedForKeys().filter((i) => i.class === "3d" || i.class === "model");
        if (items.length) void runCapture(items);
      },
    },
    {
      keys: "Ctrl+Enter",
      inFields: true,
      run: () => {
        const items = pickedForKeys().filter((i) => i.class === "3d" || i.class === "model");
        if (items.length) void startMake(items);
      },
    },
    {
      keys: "Ctrl+Shift+Enter",
      inFields: true,
      run: () => {
        const items = pickedForKeys();
        if (items.length === 1) void runRewrite(items[0]);
      },
    },
    { keys: "Ctrl+E", inFields: true, run: () => (shownResult() ? void runExport() : false) },
    {
      keys: "B",
      run: () => {
        const f = document.activeElement;
        if (!(f instanceof HTMLElement) || !f.closest(".capture-preview")) return false;
        setPreviewBg(BGS[(BGS.indexOf(previewBg) + 1) % BGS.length]);
      },
    },
    { keys: "ArrowRight", run: inGrid(() => moveFocus(1, false)) },
    { keys: "ArrowLeft", run: inGrid(() => moveFocus(-1, false)) },
    { keys: "ArrowDown", run: inGrid(() => moveFocus(gridColumns($("list-inner")), false)) },
    { keys: "ArrowUp", run: inGrid(() => moveFocus(-gridColumns($("list-inner")), false)) },
    { keys: "Shift+ArrowRight", run: inGrid(() => moveFocus(1, true)) },
    { keys: "Shift+ArrowLeft", run: inGrid(() => moveFocus(-1, true)) },
    { keys: "Home", run: inGrid(() => moveFocus("home", false)) },
    { keys: "End", run: inGrid(() => moveFocus("end", false)) },
    { keys: "Enter", run: inGrid(() => $("body-main").focus()) },
    {
      keys: "Escape",
      inFields: true,
      run: () => {
        if (document.activeElement === find && find.value) {
          find.value = "";
          setFilter({ text: "" });
          return;
        }
        if (running()) {
          void askCancel();
          return;
        }
        return false;
      },
    },
    { keys: "Ctrl+,", run: openSettings, inFields: true },
    { keys: "Ctrl+Shift+D", run: toggleTheme, inFields: true },
  ]);
  window.addEventListener("resize", fitStatusbar);
  // Lock file and branch again every 30 s while the window has the focus
  window.setInterval(() => {
    if (document.hasFocus() && !scanning && !busyNow()) void refreshProjects(false);
  }, PROBE_MS);
}

/** A folder dropped on the window is added as a project. */
async function bindDrop() {
  if (!inTauri) return;
  await getCurrentWebview().onDragDropEvent((event) => {
    const payload = event.payload;
    if (payload.type === "enter" || payload.type === "over") {
      if (!dropHover) {
        dropHover = true;
        if (!current()) renderList();
      }
    } else if (payload.type === "leave") {
      dropHover = false;
      if (!current()) renderList();
    } else if (payload.type === "drop") {
      dropHover = false;
      const path = payload.paths[0];
      if (path) void addPath(path);
      else renderList();
    }
  });
}

/** Closing while a run or a capture goes asks first; Unity in batch mode keeps running either way. */
async function bindClose() {
  if (!inTauri) return;
  const win = getCurrentWindow();
  let asking = false;
  await win.onCloseRequested(async (event) => {
    if (!busyNow()) return;
    event.preventDefault();
    if (asking || isDialogOpen()) return;
    asking = true;
    const capturing = cap.running || run?.items.some((i) => i.steps.capture.state === "run");
    const batch = current()?.mode !== "attach";
    const ok = await showDialog({
      title: t("close.title"),
      message: capturing && batch ? t("close.batch") : t("close.running"),
      confirmLabel: t("close.quit"),
      cancelLabel: t("close.stay"),
      danger: true,
    });
    asking = false;
    if (!ok) return;
    if (running()) await runCancel().catch(() => false);
    await win.destroy();
  });
}

// ---------------------------------------------------------------- boot

/** Browser preview only (README screenshots and checks). */
function applyPreviewQuery() {
  const query = new URLSearchParams(location.search);
  const tab = query.get("nav");
  if (tab === "closed") nav.setOpen(false);
  else if (tab) nav.show(tab);
  if (query.get("body") === "hidden") bodyOpen = false;
  if (query.has("sample")) currentId = query.get("project") ?? currentId ?? "p000000000001";
  const frame = /^(\d+)x(\d+)$/.exec(query.get("frame") ?? "");
  if (frame) {
    document.body.style.width = `${frame[1]}px`;
    document.body.style.height = `${frame[2]}px`;
  }
}

let previewPicked = false;
/** Browser preview: `?pick=2` picks the second shown prefab, `?pick=1,2,3` several. */
function applyPreviewPick() {
  if (inTauri || previewPicked || !scan) return;
  previewPicked = true;
  const pick = new URLSearchParams(location.search).get("pick");
  if (!pick) return;
  const order = applyFilter(scan.prefabs, filter);
  const keys = pick
    .split(",")
    .map((n) => order[Number(n) - 1])
    .filter(Boolean)
    .map(itemKey);
  selected = new Set(keys);
  focusKey = anchor = keys[0] ?? null;
}

/** After the next paint. A window that is still hidden may get no animation frame: a timer ends the wait too. */
function afterPaint(): Promise<void> {
  return new Promise((resolve) => {
    requestAnimationFrame(() => setTimeout(resolve, 0));
    setTimeout(resolve, 100);
  });
}

async function boot() {
  if (!inTauri) applyPreviewQuery();
  app.dataset.body = bodyOpen ? "open" : "hidden";
  receiveSettings(await call("settings_get").catch(() => null));
  appInfo = await call<AppInfo>("app_info").catch(() => null);
  const about = aboutPanel((text) => flashStatus(text, 5000));
  settingsDialog.addPanel("about", about.element);
  settingsDialog.onOpen(about.refresh);
  onSettingsChanged(SETTING_KEYS, true);
  onSettingsChange((changed) => onSettingsChanged(changed));
  onSettingsSaveError((error) => {
    flashStatus(t("status.saveFailed", { text: error }), 5000);
    void call("settings_get").then(receiveSettings).catch(() => {});
  });
  gen = emptyGen(getSetting("llm.rewriter") as string);
  bindEvents();
  await loadProjects();
  packShown = current()?.packId ?? packShown;
  render();
  requestAnimationFrame(() => requestAnimationFrame(() => app.classList.remove("no-anim")));
  // The window starts hidden; it shows once this first paint is ready (no white page flashes)
  await afterPaint();
  await call("window_ready").catch(() => {});
  await bindDrop();
  await bindClose();
  if (currentId) void runScan(false);
  void checkRecovery();
  void checkReady(false);
  void loadRuns();
  onTabShown();
}

void boot();
