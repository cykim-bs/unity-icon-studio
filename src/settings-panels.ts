/**
 * Parts of the settings popup the schema in settings.ts cannot draw: the About facts (versions,
 * the tool folder and the settings file, a problem reading it). Static texts carry data-t keys so a
 * language change relabels them; the panel reads its state again on `refresh()`, which runs when
 * the popup opens. After unity-slot-widget src/settings-panels.ts (MIT, same author).
 */

import { call } from "./backend";
import { t } from "./i18n";
import type { AppInfo } from "./types";

export interface Panel {
  element: HTMLElement;
  refresh(): void;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className = "", key = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (key) {
    node.dataset.t = key;
    node.textContent = t(key);
  }
  return node;
}

/** About: version, core, tool folder (with an open button), settings file. */
export function aboutPanel(onError: (text: string) => void): Panel {
  const element = el("dl", "about-facts");
  const row = (key: string): HTMLElement => {
    element.append(el("dt", "", key));
    const dd = el("dd", "setting-path");
    element.append(dd);
    return dd;
  };
  const version = row("settings.about.version");
  const core = row("settings.about.core");
  const home = row("settings.about.home");
  const file = row("settings.about.file");
  const problem = el("p", "setting-hint warn-text");
  problem.hidden = true;
  const open = el("button", "link-btn", "settings.openFolder");
  open.type = "button";
  let homePath = "";
  open.addEventListener("click", () => {
    if (homePath) void call("open_path", { path: homePath }).catch((e) => onError(t("status.openFailed", { text: String(e) })));
  });
  const wrap = el("div", "setting-facts");
  wrap.append(element, open, problem);

  return {
    element: wrap,
    refresh() {
      void call<AppInfo>("app_info")
        .then((info) => {
          if (!info) return;
          version.textContent = info.version + (info.dev ? " (dev)" : "");
          core.textContent = info.coreVersion;
          homePath = info.home;
          home.textContent = info.homeOverride ? `${info.home} · ${t("settings.about.homeOverride")}` : info.home;
          file.textContent = info.settingsFile;
          problem.hidden = !info.settingsProblem;
          problem.textContent = info.settingsProblem ? t("settings.about.problem", { text: info.settingsProblem }) : "";
        })
        .catch(() => {});
    },
  };
}
