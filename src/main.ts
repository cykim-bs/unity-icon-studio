/**
 * Main window. The frame comes from unity-slot-widget (src/shell): title bar strips, a side panel
 * with square tabs (projects, runs, styles), the prefab grid in the free width, the preview and the
 * make controls on the right, a banner for what needs a person now, and a status bar. Notices show
 * in the status bar for a moment (flashStatus) instead of toasts.
 *
 * The screen state (side panel open and tab, column width) comes back on the next start
 * (localStorage, shell/prefs). Settings live in settings.json in the tool folder (Rust side).
 */

import { call, inTauri } from "./backend";
import { applyDomTexts, resolveLang, setLang, t } from "./i18n";
import { aboutPanel } from "./settings-panels";
import { getSetting, onSettingsChange, onSettingsSaveError, receiveSettings, setSetting, SETTING_KEYS, type SettingKey } from "./settings";
import { relabelBanner, setBannerFocusHome } from "./shell/banner";
import { initFrame } from "./shell/frame";
import { initKeys } from "./shell/keys";
import { initNav } from "./shell/nav";
import { isBoolean, readPref } from "./shell/prefs";
import { initBodyResize } from "./shell/resize";
import { initSettingsDialog } from "./shell/settings-dialog";
import { fitStatusbar, flashStatus, setStatusPath } from "./shell/statusbar";
import { initTheme, isThemeChoice, oppositeChoice } from "./shell/theme";
import { initTitlebar } from "./shell/titlebar";
import type { AppInfo } from "./types";

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

let appInfo: AppInfo | null = null;
let bodyOpen = readPref("bodyOpen", true, isBoolean);

// ---------------------------------------------------------------- drawing

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className = "", text = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
}

function renderProjects() {
  const body = $("projects-body");
  body.replaceChildren(el("p", "panel-empty", t("projects.none")));
}

/** Middle column with no project: what to do first. */
function renderList() {
  $("list-tools").hidden = true;
  $("project-title").textContent = t("app.title");
  const box = el("div", "empty-state");
  box.append(el("h2", "", t("empty.title")));
  const add = el("button", "act-btn", t("projects.add"));
  add.type = "button";
  add.addEventListener("click", addProject);
  box.append(add, el("p", "muted", t("empty.drop")));
  $("list-inner").replaceChildren(box);
}

/** Right column with nothing chosen: the readiness checklist (filled in once the checks exist). */
function renderBody() {
  const content = $("body-content");
  const list = el("div", "ready-list");
  list.append(el("h3", "", t("ready.title")));
  for (const key of ["ready.codex", "ready.claude", "ready.unityCli", "ready.unity"]) {
    const row = el("div", "ready-row");
    row.append(el("span", "ready-dot"), el("span", "ready-name", t(key)), el("span", "muted", t("ready.notChecked")));
    list.append(row);
  }
  content.replaceChildren(list);
}

function renderStatus() {
  setStatusPath(appInfo ? appInfo.home : "");
  $("status-mode").textContent = "";
  $("status-ready").textContent = "";
  $("status-today").textContent = t("status.today", { n: 0 });
  $("status-version").textContent = appInfo ? t("status.version", { version: appInfo.version }) : "";
  fitStatusbar();
}

function render() {
  renderProjects();
  renderList();
  renderBody();
  renderStatus();
}

// ---------------------------------------------------------------- actions

function addProject() {
  flashStatus(t("status.notYet"));
}

function setBodyOpen(open: boolean) {
  bodyOpen = open;
  app.dataset.body = open ? "open" : "hidden";
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
  $("project-add").addEventListener("click", addProject);
  $("status-version").addEventListener("click", () => settingsDialog.open("about"));
  setBannerFocusHome(() => $("list").focus());
  initBodyResize({ fold: () => setBodyOpen(false) });
  initKeys([
    { keys: "Ctrl+1", run: () => nav.show("projects") },
    { keys: "Ctrl+2", run: () => nav.show("runs") },
    { keys: "Ctrl+3", run: () => nav.show("packs") },
    { keys: "Ctrl+B", run: () => nav.toggle(), inFields: true },
    { keys: "Ctrl+\\", run: () => setBodyOpen(!bodyOpen), inFields: true },
    { keys: "Ctrl+O", run: addProject },
    { keys: "Ctrl+,", run: openSettings, inFields: true },
    { keys: "Ctrl+Shift+D", run: toggleTheme, inFields: true },
  ]);
  window.addEventListener("resize", fitStatusbar);
}

// ---------------------------------------------------------------- boot

/** Browser preview only (README screenshots and checks). */
function applyPreviewQuery() {
  const query = new URLSearchParams(location.search);
  const tab = query.get("nav");
  if (tab === "closed") nav.setOpen(false);
  else if (tab) nav.show(tab);
  if (query.get("body") === "hidden") bodyOpen = false;
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
  setBodyOpen(bodyOpen);
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
  render();
  requestAnimationFrame(() => requestAnimationFrame(() => app.classList.remove("no-anim")));
  // The window starts hidden; it shows once this first paint is ready (no white page flashes)
  await afterPaint();
  await call("window_ready").catch(() => {});
}

void boot();
