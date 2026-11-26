#!/usr/bin/env node
// Drives the real app window for end-to-end checks: presses buttons, types, reads the screen and
// takes screenshots through the WebView2 remote debugging port. Development use only. The port is
// open only when the app was started with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS (the `launch`
// command sets it for that one child process); an app started normally has no port. Node 22+
// (global WebSocket and fetch), no packages.
//
//   node scripts/drive-window.mjs launch <app.exe> [--home <dir>] [--env NAME=value ...]
//       --home: the tool home for this run (UNITY_ICON_STUDIO_HOME) and a WebView profile in it
//   node scripts/drive-window.mjs state
//   node scripts/drive-window.mjs click "#make-btn"          (or: click --text "Make icon")
//   node scripts/drive-window.mjs type "#make-note" "snow on the roof"
//   node scripts/drive-window.mjs select "#settings-body select" auto
//   node scripts/drive-window.mjs key Ctrl+Enter
//   node scripts/drive-window.mjs wait "#result" [--text <text>] [--timeout 120]
//   node scripts/drive-window.mjs text "#statusbar"
//   node scripts/drive-window.mjs eval "document.title"
//   node scripts/drive-window.mjs shot out.png
//   node scripts/drive-window.mjs run steps.json [--var NAME=value ...]
//       steps: [{ "do": "click|type|select|key|wait|text|expect|eval|state|shot|sleep|write|close", ... }]
//       (sel, text, value, keys, js, file, timeout, gone, ctrl, shift; ${NAME} takes a --var or an
//       environment variable). scripts/window-e2e.json is the whole stub flow.
//   node scripts/drive-window.mjs close
//
// Options: --port <n> (default 9333, or UIS_DRIVE_PORT). Each command prints one JSON line.
// Native windows (the folder picker) cannot be driven: add projects with 「Type a path…」 in the
// add menu, and set the export folder in the settings or the tool's settings.json.

import { spawn } from "node:child_process";
import { writeFileSync, readFileSync } from "node:fs";
import { resolve } from "node:path";

const argv = process.argv.slice(2);
function option(name, fallback = null) {
  const i = argv.indexOf(name);
  if (i < 0) return fallback;
  const v = argv[i + 1];
  argv.splice(i, 2);
  return v;
}
function options(name) {
  const out = [];
  for (let v = option(name); v !== null; v = option(name)) out.push(v);
  return out;
}
const port = Number(option("--port", process.env.UIS_DRIVE_PORT ?? "9333"));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ---------------------------------------------------------------- DevTools connection

async function pageSocket(timeoutMs = 15000) {
  const end = Date.now() + timeoutMs;
  let last = "";
  while (Date.now() < end) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      const page = list.find((p) => p.type === "page");
      if (page?.webSocketDebuggerUrl) return page.webSocketDebuggerUrl;
      last = "no page yet";
    } catch (e) {
      last = String(e.cause?.code ?? e.message ?? e);
    }
    await sleep(300);
  }
  throw new Error(`no app window on port ${port} (${last}); start it with \`launch\``);
}

class Cdp {
  static async open() {
    const ws = new WebSocket(await pageSocket());
    await new Promise((ok, fail) => {
      ws.addEventListener("open", ok, { once: true });
      ws.addEventListener("error", () => fail(new Error("websocket")), { once: true });
    });
    return new Cdp(ws);
  }
  constructor(ws) {
    this.ws = ws;
    this.next = 1;
    this.waiting = new Map();
    ws.addEventListener("message", (m) => {
      const msg = JSON.parse(m.data);
      const w = msg.id && this.waiting.get(msg.id);
      if (!w) return;
      this.waiting.delete(msg.id);
      if (msg.error) w.fail(new Error(msg.error.message));
      else w.ok(msg.result);
    });
  }
  send(method, params = {}) {
    const id = this.next++;
    this.ws.send(JSON.stringify({ id, method, params }));
    return new Promise((ok, fail) => this.waiting.set(id, { ok, fail }));
  }
  /** Runs `js` in the page (await allowed) and returns its JSON value. */
  async eval(js) {
    const r = await this.send("Runtime.evaluate", { expression: `(async () => { ${js} })()`, awaitPromise: true, returnByValue: true, userGesture: true });
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
    return r.result.value;
  }
  close() {
    this.ws.close();
  }
}

// ---------------------------------------------------------------- page helpers (run inside the page)

const FIND = `
  const find = (sel, text) => {
    const nodes = sel ? [...document.querySelectorAll(sel)] : [...document.querySelectorAll('button, [role="tab"], [role="option"], a, label, li')];
    const seen = (n) => n.offsetParent !== null || n.getClientRects().length > 0;
    const hit = nodes.filter((n) => seen(n) && (!text || (n.textContent || '').replace(/\\s+/g, ' ').includes(text)));
    return hit[0] ?? null;
  };`;

const STATE = `
  const q = (s) => document.querySelector(s);
  const txt = (s) => (q(s)?.textContent ?? '').replace(/\\s+/g, ' ').trim();
  const dialog = q('dialog[open]');
  return {
    title: txt('#project-title'),
    body: txt('#body-title'),
    tab: q('#nav-tabs [aria-selected="true"]')?.dataset.tab ?? null,
    banner: q('#banner')?.hidden ? null : txt('#banner-text'),
    status: { progress: txt('#status-progress-text'), ready: txt('#status-ready'), today: txt('#status-today'), notice: txt('#status-live') },
    projects: [...document.querySelectorAll('#projects-body .project-row, #projects-body li')].map((n) => n.textContent.replace(/\\s+/g, ' ').trim()).slice(0, 10),
    grid: { cells: document.querySelectorAll('.prefab-cell[data-key]').length, selected: [...document.querySelectorAll('.prefab-cell.selected')].map((n) => n.dataset.key), count: txt('#list-count') },
    steps: [...document.querySelectorAll('.stepper .step')].map((n) => n.dataset.step + ':' + n.dataset.state),
    making: !!q('#cancel-btn'),
    fail: txt('.fail-box') || null,
    result: q('#result') ? { checks: txt('.checks-head'), cands: [...document.querySelectorAll('.cand-tab')].map((n) => n.textContent.trim()), exported: txt('.export-done') || null } : null,
    runList: [...document.querySelectorAll('#run-list .run-row')].map((n) => n.textContent.replace(/\\s+/g, ' ').trim()),
    runs: [...document.querySelectorAll('.runs-list .run-item')].map((n) => n.textContent.replace(/\\s+/g, ' ').trim()).slice(0, 10),
    dialog: dialog ? { id: dialog.id, title: dialog.querySelector('h2')?.textContent ?? '', message: dialog.querySelector('#dialog-message')?.textContent ?? '' } : null,
  };`;

const KEYS = { Enter: 13, Escape: 27, Tab: 9, Backspace: 8, ArrowUp: 38, ArrowDown: 40, ArrowLeft: 37, ArrowRight: 39, Home: 36, End: 35, F5: 116, Space: 32 };

async function pressKey(cdp, combo) {
  const parts = combo.split("+");
  const name = parts.pop();
  const mods = new Set(parts.map((p) => p.toLowerCase()));
  const modifiers = (mods.has("alt") ? 1 : 0) | (mods.has("ctrl") ? 2 : 0) | (mods.has("shift") ? 8 : 0);
  let key = name;
  let code = name;
  let vk = KEYS[name] ?? 0;
  if (/^[a-z]$/i.test(name)) {
    key = mods.has("shift") ? name.toUpperCase() : name.toLowerCase();
    code = `Key${name.toUpperCase()}`;
    vk = name.toUpperCase().charCodeAt(0);
  } else if (/^[0-9]$/.test(name)) {
    code = `Digit${name}`;
    vk = name.charCodeAt(0);
  } else if (name === ",") {
    code = "Comma";
    vk = 188;
  }
  const base = { modifiers, key, code, windowsVirtualKeyCode: vk };
  await cdp.send("Input.dispatchKeyEvent", { type: "rawKeyDown", ...base });
  await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", ...base });
}

// ---------------------------------------------------------------- commands

async function step(cdp, s, vars) {
  const sub = (v) => (typeof v === "string" ? v.replace(/\$\{(\w+)\}/g, (_, k) => vars[k] ?? process.env[k] ?? "") : v);
  const sel = sub(s.sel ?? null);
  const text = sub(s.text ?? null);
  switch (s.do) {
    case "click": {
      // ctrl / shift: a click with that key held (several prefabs in the grid)
      const mods = JSON.stringify({ ctrlKey: !!s.ctrl, shiftKey: !!s.shift });
      // Menus and dialogs come a moment after the press that opens them: try for a few seconds
      const end = Date.now() + (s.timeout ?? 5) * 1000;
      let ok = false;
      while (!ok && Date.now() < end) {
        ok = await cdp.eval(
          `${FIND} const n = find(${JSON.stringify(sel)}, ${JSON.stringify(text)}); if (!n || n.disabled) return false; n.scrollIntoView({ block: 'center' }); const m = ${mods}; if (m.ctrlKey || m.shiftKey) n.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, ...m })); else n.click(); return true;`,
        );
        if (!ok) await sleep(200);
      }
      if (!ok) throw new Error(`nothing to click: ${sel ?? ""} ${text ?? ""}`);
      return { clicked: sel ?? text };
    }
    case "type": {
      const ok = await cdp.eval(
        `const n = document.querySelector(${JSON.stringify(sel)}); if (!n) return false; n.focus(); n.value = ${JSON.stringify(text ?? "")}; n.dispatchEvent(new Event('input', { bubbles: true })); n.dispatchEvent(new Event('change', { bubbles: true })); return true;`,
      );
      if (!ok) throw new Error(`no field ${sel}`);
      return { typed: sel };
    }
    case "select": {
      const value = sub(s.value);
      const ok = await cdp.eval(`const n = document.querySelector(${JSON.stringify(sel)}); if (!n) return false; n.value = ${JSON.stringify(value)}; n.dispatchEvent(new Event('change', { bubbles: true })); return n.value === ${JSON.stringify(value)};`);
      if (!ok) throw new Error(`cannot select ${value} in ${sel}`);
      return { selected: value };
    }
    case "key":
      await pressKey(cdp, sub(s.keys));
      return { key: s.keys };
    case "wait": {
      const end = Date.now() + (s.timeout ?? 30) * 1000;
      const gone = !!s.gone;
      while (Date.now() < end) {
        const hit = await cdp.eval(`${FIND} return !!find(${JSON.stringify(sel)}, ${JSON.stringify(text)});`);
        if (hit !== gone) return { waited: sel ?? text, gone };
        await sleep(250);
      }
      throw new Error(`timed out waiting for ${sel ?? ""} ${text ?? ""}${gone ? " to go" : ""}`);
    }
    case "write": {
      // A file outside the app (a stub scenario between two runs)
      const file = resolve(sub(s.file));
      writeFileSync(file, sub(s.text ?? "").replace(/\\n/g, "\n"));
      return { wrote: file };
    }
    case "sleep":
      await sleep(s.ms ?? 500);
      return { slept: s.ms ?? 500 };
    case "eval":
      return { value: await cdp.eval(sub(s.js).includes("return") ? sub(s.js) : `return (${sub(s.js)});`) };
    case "text":
      return { text: await cdp.eval(`return (document.querySelector(${JSON.stringify(sel)})?.textContent ?? '').replace(/\\s+/g, ' ').trim();`) };
    case "expect": {
      const got = await cdp.eval(`${FIND} const n = find(${JSON.stringify(sel)}, null); return n ? n.textContent.replace(/\\s+/g, ' ').trim() : null;`);
      if (got === null || (text && !got.includes(text))) throw new Error(`expected ${sel} to contain ${JSON.stringify(text)}, got ${JSON.stringify(got)}`);
      return { ok: sel };
    }
    case "state":
      return await cdp.eval(STATE);
    case "shot": {
      const file = resolve(sub(s.file));
      const r = await cdp.send("Page.captureScreenshot", { format: "png", captureBeyondViewport: false });
      writeFileSync(file, Buffer.from(r.data, "base64"));
      return { shot: file };
    }
    case "close":
      await cdp.eval(`document.querySelector('#win-close')?.click(); return true;`);
      return { closing: true };
    default:
      throw new Error(`unknown step ${s.do}`);
  }
}

async function launch(exe, home, envs) {
  const env = { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}` };
  if (home) {
    env.UNITY_ICON_STUDIO_HOME = resolve(home);
    // A WebView profile of its own next to that home: a clean screen state (filters, widths),
    // and the person's own app profile is left alone
    env.WEBVIEW2_USER_DATA_FOLDER = resolve(home, "webview");
  }
  for (const e of envs) {
    const i = e.indexOf("=");
    if (i > 0) env[e.slice(0, i)] = e.slice(i + 1);
  }
  const child = spawn(resolve(exe), [], { env, detached: true, stdio: "ignore" });
  child.unref();
  await pageSocket(30000);
  const cdp = await Cdp.open();
  // The page has drawn once the window shows its project title
  for (let i = 0; i < 60; i++) {
    if (await cdp.eval(`return !!document.querySelector('#project-title')?.textContent;`).catch(() => false)) break;
    await sleep(250);
  }
  cdp.close();
  return { launched: resolve(exe), pid: child.pid, port };
}

async function main() {
  const vars = Object.fromEntries(options("--var").map((v) => [v.slice(0, v.indexOf("=")), v.slice(v.indexOf("=") + 1)]));
  const [cmd, ...rest] = argv;
  if (!cmd || cmd === "help" || cmd === "--help") {
    process.stdout.write(readFileSync(new URL(import.meta.url), "utf8").split("\n").filter((l) => l.startsWith("//")).map((l) => l.slice(3)).join("\n") + "\n");
    return;
  }
  if (cmd === "launch") {
    const home = option("--home");
    const envs = options("--env");
    console.log(JSON.stringify(await launch(argv[1], home, envs)));
    return;
  }
  const cdp = await Cdp.open();
  try {
    if (cmd === "run") {
      const steps = JSON.parse(readFileSync(rest[0], "utf8"));
      const started = Date.now();
      for (const [i, s] of steps.entries()) {
        const t = Date.now();
        try {
          const out = await step(cdp, s, vars);
          console.log(JSON.stringify({ step: i + 1, do: s.do, note: s.note, ms: Date.now() - t, ...out }));
        } catch (e) {
          console.log(JSON.stringify({ step: i + 1, do: s.do, note: s.note, error: String(e.message ?? e) }));
          console.log(JSON.stringify({ state: await step(cdp, { do: "state" }, vars).catch(() => null) }));
          process.exitCode = 1;
          return;
        }
      }
      console.log(JSON.stringify({ done: steps.length, ms: Date.now() - started }));
      return;
    }
    const text = option("--text");
    const timeout = option("--timeout");
    const s =
      cmd === "click" ? { do: "click", sel: rest[0] ?? null, text }
      : cmd === "type" ? { do: "type", sel: rest[0], text: rest[1] }
      : cmd === "select" ? { do: "select", sel: rest[0], value: rest[1] }
      : cmd === "key" ? { do: "key", keys: rest[0] }
      : cmd === "wait" ? { do: "wait", sel: rest[0] ?? null, text, timeout: timeout ? Number(timeout) : 30 }
      : cmd === "text" ? { do: "text", sel: rest[0] }
      : cmd === "eval" ? { do: "eval", js: rest[0] }
      : cmd === "shot" ? { do: "shot", file: rest[0] }
      : cmd === "state" ? { do: "state" }
      : cmd === "close" ? { do: "close" }
      : null;
    if (!s) throw new Error(`unknown command ${cmd}`);
    console.log(JSON.stringify(await step(cdp, s, vars)));
  } finally {
    cdp.close();
  }
}

main().catch((e) => {
  console.log(JSON.stringify({ error: String(e.message ?? e) }));
  process.exitCode = 1;
});
