# Third-party notices

## Window frame (unity-slot-widget, Frond)

The window frame in `src/shell/` (title bar, side panel, column resizing, menus, dialogs, banner,
status bar, themes, settings popup, shortcuts), its line icons (`src/shell/icons.ts` and the
caption and side panel icons in `index.html`), parts of `src/settings.ts`, `src/i18n.ts`,
`scripts/check-i18n.mjs`, `scripts/launch-user.ps1`, `scripts/build-release.ps1`,
`scripts/capture-window.ps1` and `scripts/make_icon.py` were copied from
[unity-slot-widget](https://github.com/cyKim0115/unity-slot-widget) @70169e4 (MIT License), whose
frame was adapted from [Frond](https://github.com/cyKim0115/Frond/) @99d89bc (MIT License).
Per-file sources and changes are listed in `src/shell/SOURCES.md`.
The Frond and unity-slot-widget names and app icons are not used.

The app icon (`app-icon.png`, `src-tauri/icons/`) was drawn for this app from basic shapes
(`scripts/draw_app_icon.py`).

## Trademarks

Unity is a trademark of Unity Technologies. This project is not affiliated with Unity, OpenAI or
Anthropic. It does not include or redistribute Unity, Codex or Claude: it runs the copies installed
on your PC.

## Libraries

The app is built with [Tauri](https://tauri.app) (MIT or Apache-2.0) and the Rust crates and npm
packages listed in `Cargo.toml`, `crates/*/Cargo.toml`, `src-tauri/Cargo.toml`, `Cargo.lock`,
`package.json` and `package-lock.json`, each under its own license. Among them:

| Package | License | Use |
|---|---|---|
| `serde`, `serde_json` | MIT or Apache-2.0 | settings and data files |
| `windows` | MIT or Apache-2.0 | Job Objects for the external programs' time limit |
| `tauri-plugin-window-state` | MIT or Apache-2.0 | remembers window position and size |
| `tauri-plugin-single-instance` | MIT or Apache-2.0 | a second launch brings the running window forward |
| `tauri-plugin-dialog` | MIT or Apache-2.0 | folder picker |
| `tauri-plugin-opener` | MIT or Apache-2.0 | opens folders in Explorer |
| `vitest` | MIT | unit tests (development only, not shipped) |
| `jsdom` | MIT | DOM for the unit tests (development only, not shipped) |
