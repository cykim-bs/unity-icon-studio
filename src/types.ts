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

/** How a capture would run now (uis-core project.rs probe), with the list entry. */
export interface ProjectSummary {
  id: string;
  packId: string;
  exists: boolean;
  mode: "attach" | "batch" | "blocked";
  reason: string;
  lock: "locked" | "stale" | "none";
  path: string;
  name: string;
  unityVersion: string | null;
  unityRevision: string | null;
  urp: boolean;
  pipelinePackage: boolean;
  branch: string | null;
  isGit: boolean;
}

export type PrefabClass = "3d" | "ui" | "2d" | "fx" | "empty" | "unknown" | "model";
export type Kind = "building" | "prop" | "character" | "generic";

export interface PrefabItem {
  guid: string;
  path: string;
  name: string;
  folder: string;
  class: PrefabClass;
  kind: Kind;
  kindSource: "rule" | "skinned" | "default" | "override";
  rule?: string;
  skinned: boolean;
  nested: number;
  mtimeMs: number;
}

export interface ScanResult {
  schema: string;
  project: string;
  packId: string;
  total: number;
  counts: Record<string, number>;
  kinds: Record<string, number>;
  metas: number;
  models: number;
  elapsedMs: number;
  cache: { metasRead: number; metasCached: number; prefabsRead: number; prefabsCached: number; usedCacheFile: boolean };
  prefabs: PrefabItem[];
}

export interface HubProject {
  path: string;
  title: string;
  version: string | null;
  lastModified: number | null;
  favorite: boolean;
  exists: boolean;
}

export interface PackSummary {
  id: string;
  name: Record<string, string> | string | null;
  source: "builtin" | "local";
  extends: string | null;
  kindRules: number;
  styleRefs: number;
  dir: string | null;
  problem: string | null;
}
