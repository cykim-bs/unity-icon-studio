/** Shapes the Rust side sends (crates/uis-core, src-tauri/src/commands.rs). */

export interface AppInfo {
  version: string;
  coreVersion: string;
  home: string;
  homeOverride: boolean;
  settingsFile: string;
  settingsProblem: string | null;
  dev: boolean;
}
