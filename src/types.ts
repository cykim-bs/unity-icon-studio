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

/** A step of a capture (uis-core capture.rs CaptureEvent). */
export interface CaptureEvent {
  stage: "probe" | "hooks" | "unity" | "cleanup" | "done";
  message: string;
  done: number;
  total: number;
  elapsedMs: number;
}

export interface CaptureItemReport {
  id: string;
  prefab: string;
  kind: Kind;
  ok: boolean;
  error?: string;
  output?: string;
  thumb?: string;
  coverage: number | null;
}

/** What a capture request did (uis-core capture.rs CaptureReport, trimmed to what the page reads). */
export interface CaptureReport {
  runId: string;
  mode: "attach" | "batch" | "blocked";
  reason: string;
  project: string;
  outDir: string;
  ok: boolean;
  error: string | null;
  items: CaptureItemReport[];
  failed: number;
  exitCode?: number;
  pending: boolean;
  logTail: string[];
  unityMs: number;
  elapsedMs: number;
}

export interface RecoveryEntry {
  runId: string;
  project: string;
  state: string;
  pid: number;
  running: boolean;
  startedMs: number;
}

/** A finished left-over batch capture (uis-core capture.rs BatchFinish). */
export interface BatchFinish {
  runId: string;
  project: string;
  exitCode: number | null;
  entryRemoved: boolean;
  error: string | null;
}

// ---------------------------------------------------------------- prompt and CLIs (cli/*.rs, prompt.rs, generate.rs)

export interface CliState {
  found: boolean;
  path: string | null;
  source: string | null;
  version: string | null;
  loggedIn: boolean | null;
  imageTool: boolean | null;
  enabled: boolean;
  problem: string | null;
}

export interface Readiness {
  codex: CliState;
  claude: CliState;
  unityCli: CliState;
  codexApp: { version: string; resources: string; bytes: number } | null;
  codexCopyNeeded: boolean;
  elapsedMs: number;
}

export interface CopyInfo {
  appVersion: string;
  cliVersion: string | null;
  dir: string;
  bytes: number;
}

export type SentenceStatus = "same" | "replaced" | "added";

export interface Sentence {
  id: string;
  block: string;
  locked: boolean;
  text: string;
  status: SentenceStatus;
  before?: string;
}

export interface Rejected {
  part: string;
  reason: string;
  /** model: the LLM left it out; code: the tool took it out */
  by: string;
}

export interface Failure {
  kind: string;
  cli: string;
  message: string;
  exitCode: number | null;
}

export interface LlmCall {
  cli: string;
  purpose: string;
  attempt: number;
  elapsedMs: number;
  code: string;
}

export interface PromptDiff {
  changed: boolean;
  sentences: Sentence[];
  rejected: Rejected[];
  summaryKo: string;
  outline: string;
  camera: string;
}

export interface RewriteReport {
  ok: boolean;
  note: string;
  cli: string | null;
  fallbackFrom: string | null;
  finalText: string;
  diff: PromptDiff;
  calls: LlmCall[];
  failure: Failure | null;
  choices: string[];
}

export interface ImageResult {
  ok: boolean;
  raw: string | null;
  harvestStage: number | null;
  width: number;
  height: number;
  hasAlpha: boolean;
  failure: Failure | null;
  call: LlmCall;
}

export interface GenerateReport {
  ok: boolean;
  jobDir: string;
  finalText: string;
  rewrite: RewriteReport | null;
  candidates: ImageResult[];
  error: string | null;
  elapsedMs: number;
}
