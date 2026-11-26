/**
 * Main window. The frame comes from unity-slot-widget (src/shell): title bar strips, a side panel
 * with square tabs (projects, runs, styles), the prefab grid in the free width, the preview and the
 * make controls on the right, a banner for what needs a person now, and a status bar. Notices show
 * in the status bar for a moment (flashStatus) instead of toasts.
 *
 * Projects: added with the folder picker (Ctrl+O), by dropping a folder on the window, or from
 * Unity Hub's list. Picking one reads its prefabs without Unity (uis-core prefabs.rs, cached) and
 * checks how a capture would run (lock file, version, packages). The state is checked again every
 * 30 seconds while the window has the focus.
 *
 * The screen state (side panel, widths, project, filters) comes back on the next start
 * (localStorage, shell/prefs). Settings live in settings.json in the tool folder (Rust side).
 */

import { getCurrentWebview } from "@tauri-apps/api/webview";
import { call, captureImage, captureOnly, inTauri } from "./backend";
import { captureErrorText } from "./capture-text";
import { applyDomTexts, currentLang, resolveLang, setLang, t } from "./i18n";
import { applyFilter, CLASS_CHIPS, DEFAULT_FILTER, folderChoices, KIND_CHIPS, nextSelection, type ClassChip, type PrefabFilter } from "./prefab-filter";
import { renderDetails, type CaptureView } from "./screens/details";
import { gridColumns, itemKey, renderChips, renderGrid } from "./screens/prefabs";
import { modeText, reasonText, renderProjects } from "./screens/projects";
import { aboutPanel } from "./settings-panels";
import { getSetting, onSettingsChange, onSettingsSaveError, receiveSettings, setSetting, SETTING_KEYS, type SettingKey } from "./settings";
import { relabelBanner, setBannerFocusHome, showBanner } from "./shell/banner";
import { showContextMenu, showMenuBelow, type MenuEntry } from "./shell/context-menu";
import { initFrame } from "./shell/frame";
import { initKeys } from "./shell/keys";
import { initNav } from "./shell/nav";
import { isBoolean, readPref, writePref } from "./shell/prefs";
import { initBodyResize } from "./shell/resize";
import { initSettingsDialog } from "./shell/settings-dialog";
import { fitStatusbar, flashStatus, setStatusPath } from "./shell/statusbar";
import { initTheme, isThemeChoice, oppositeChoice } from "./shell/theme";
import { initTitlebar } from "./shell/titlebar";
import type { AppInfo, BatchFinish, CaptureEvent, HubProject, Kind, PackSummary, PrefabItem, ProjectSummary, RecoveryEntry, ScanResult } from "./types";

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
const emptyCapture = (): CaptureView => ({ running: false, progress: "", error: null, images: {}, failures: {} });
/** Capture state of the open project (cleared when another project is opened). */
let cap: CaptureView = emptyCapture();

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

// ---------------------------------------------------------------- drawing

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className = "", text = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
}

function renderNav() {
  renderProjects($("projects-body"), { projects, current: currentId }, { pick: pickProject, menu: projectMenu });
  const pack = packs.filter((p) => p.source === "local");
  const packsBody = $("packs-body");
  if (!pack.length) packsBody.replaceChildren(el("p", "panel-empty", t("packs.empty")));
  else {
    const list = el("ul", "pack-list");
    for (const p of packs) {
      const li = el("li", "pack-row", `${p.id}${p.problem ? ` — ${p.problem}` : ""}`);
      list.append(li);
    }
    packsBody.replaceChildren(list);
  }
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
    add.addEventListener("click", () => void pickFolder());
    box.append(add, el("p", "muted", inTauri ? t("empty.drop") : t("empty.preview")));
    inner.replaceChildren(box);
    return;
  }
  tools.hidden = false;
  if (!scan) {
    const box = el("div", "empty-state");
    box.append(el("p", scanError ? "hot-text" : "muted", scanError ? errorText(scanError) : t("prefabs.scanning")));
    inner.replaceChildren(box);
    $("list-count").textContent = "";
    return;
  }
  shown = applyFilter(scan.prefabs, filter);
  const view = { items: shown, all: scan.prefabs, filter, selected, focus: focusKey };
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
  const title = renderDetails(
    $("body-content"),
    { project: current(), pack: packOf(current()), picked: picked(), lang: currentLang(), capture: cap },
    {
      setKind: (item, kind) => void setKind(item, kind),
      openFolder: (folder) => void openInProject(folder),
      copy: (text) => void copyText(text),
      capture: (items) => void runCapture(items),
    },
  );
  $("body-title").textContent = title || t("body.title");
}

function renderStatus() {
  const p = current();
  setStatusPath(p ? p.path : (appInfo?.home ?? ""));
  $("status-mode").textContent = p ? modeText(p) : "";
  $("status-mode").title = p ? reasonText(p) : "";
  $("status-ready").textContent = "";
  $("status-busy").hidden = !scanning && !cap.running;
  $("status-progress-text").textContent = cap.running
    ? cap.progress
    : scanning
    ? t("status.scanning")
    : scan
      ? t("status.scanned", { n: scan.total, ms: scan.elapsedMs })
      : "";
  $("status-today").textContent = t("status.today", { n: 0 });
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

// ---------------------------------------------------------------- projects

async function loadProjects() {
  projects = (await call<ProjectSummary[]>("project_list").catch(() => null)) ?? [];
  packs = (await call<PackSummary[]>("pack_list").catch(() => null)) ?? [];
  if (currentId && !projects.some((p) => p.id === currentId)) currentId = null;
  if (!currentId && projects.length) currentId = projects[0].id;
}

function pickProject(id: string) {
  if (id === currentId && scan) return;
  currentId = id;
  writePref("project", id);
  if (!cap.running) cap = emptyCapture();
  scan = null;
  scanError = null;
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
  } catch (e) {
    if (id !== currentId) return;
    scanError = String(e);
    if (scanError === "busy") flashStatus(errorText(scanError));
  } finally {
    scanning = false;
  }
  render();
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

async function hubMenu(): Promise<MenuEntry[]> {
  const hub = (await call<HubProject[]>("hub_projects").catch(() => null)) ?? [];
  const known = new Set(projects.map((p) => p.path.toLowerCase()));
  const usable = hub.filter((h) => h.exists && !known.has(h.path.toLowerCase())).slice(0, 12);
  if (!usable.length) return [{ label: t("projects.hubNone"), action: () => {}, disabled: true }];
  return usable.map((h) => ({ label: `${h.title}${h.version ? `  (${h.version})` : ""}`, action: () => void addPath(h.path) }));
}

async function addMenu(button: HTMLElement) {
  const entries: MenuEntry[] = [
    { label: t("projects.pickFolder"), shortcut: "Ctrl+O", action: () => void pickFolder() },
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
  if (!cell) return;
  const order = shown.map(itemKey);
  const next = nextSelection(order, selected, anchor, cell.dataset.key!, { ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey });
  selected = next.selected;
  anchor = next.anchor;
  focusKey = cell.dataset.key!;
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

// ---------------------------------------------------------------- capture

function progressText(e: CaptureEvent): string {
  return t("capture.progress", { stage: t(`capture.stage.${e.stage}`), done: e.done, total: e.total });
}

/** Captures the items (in the open editor or in batch mode) and shows what came out. */
async function runCapture(items: PrefabItem[]) {
  const id = currentId;
  if (!id || cap.running || !items.length) return;
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
    } else {
      cap = emptyCapture();
    }
  }
}

/** Batch captures left by an earlier run: a banner to clean them up (or one saying Unity still runs). */
async function checkRecovery() {
  const left = (await call<RecoveryEntry[]>("recovery_status").catch(() => null)) ?? [];
  if (!left.length) {
    showBanner(null);
    return;
  }
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
}

function bindEvents() {
  $("settings-btn").addEventListener("click", openSettings);
  $("project-add").addEventListener("click", (e) => void addMenu(e.currentTarget as HTMLElement));
  $("status-version").addEventListener("click", () => settingsDialog.open("about"));
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
  $("list-more").addEventListener("click", (e) =>
    showMenuBelow(e.currentTarget as HTMLElement, [
      { label: t("prefabs.rescan"), shortcut: "F5", action: () => void runScan(false) },
      { label: t("prefabs.rescanAll"), shortcut: "Shift+F5", action: () => void runScan(true) },
      "separator",
      { label: t("prefabs.resetFilter"), action: () => setFilter({ ...DEFAULT_FILTER }) },
    ]),
  );
  const find = $("find-input") as HTMLInputElement;
  find.addEventListener("input", () => setFilter({ text: find.value }));
  $("list").addEventListener("click", clickCell);
  $("list").addEventListener("dblclick", (e) => {
    if ((e.target as Element).closest(".prefab-cell")) $("body-main").focus();
  });
  setBannerFocusHome(() => $("list").focus());
  initBodyResize({ fold: () => setBodyOpen(false) });
  const inGrid = (fn: () => void) => () => gridKeysActive() && (fn(), true);
  initKeys([
    { keys: "Ctrl+1", run: () => nav.show("projects") },
    { keys: "Ctrl+2", run: () => nav.show("runs") },
    { keys: "Ctrl+3", run: () => nav.show("packs") },
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
        return false;
      },
    },
    { keys: "Ctrl+,", run: openSettings, inFields: true },
    { keys: "Ctrl+Shift+D", run: toggleTheme, inFields: true },
  ]);
  window.addEventListener("resize", fitStatusbar);
  // Lock file and branch again every 30 s while the window has the focus
  window.setInterval(() => {
    if (document.hasFocus() && !scanning) void refreshProjects(false);
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

// ---------------------------------------------------------------- boot

/** Browser preview only (README screenshots and checks). */
function applyPreviewQuery() {
  const query = new URLSearchParams(location.search);
  const tab = query.get("nav");
  if (tab === "closed") nav.setOpen(false);
  else if (tab) nav.show(tab);
  if (query.get("body") === "hidden") bodyOpen = false;
  if (query.has("sample")) currentId = query.get("project") ?? currentId;
  const frame = /^(\d+)x(\d+)$/.exec(query.get("frame") ?? "");
  if (frame) {
    document.body.style.width = `${frame[1]}px`;
    document.body.style.height = `${frame[2]}px`;
  }
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
  bindEvents();
  await loadProjects();
  render();
  requestAnimationFrame(() => requestAnimationFrame(() => app.classList.remove("no-anim")));
  // The window starts hidden; it shows once this first paint is ready (no white page flashes)
  await afterPaint();
  await call("window_ready").catch(() => {});
  await bindDrop();
  if (currentId) void runScan(false);
  void checkRecovery();
}

void boot();
