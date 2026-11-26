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
  /** Tracked files around the capture (gitguard.rs): put back for line endings only, and what the
   * capture left changed */
  git?: { lineEndings?: string[]; left?: string[]; restored?: string[] } | null;
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

/** One pixel check of an icon (checks.rs). */
export interface CheckItem {
  id: string;
  /** frame, outline, color or generate */
  group: string;
  value: number;
  min: number | null;
  max: number | null;
  pass: boolean;
}

export interface CheckReport {
  pass: boolean;
  failed: string[];
  outlineOnly: boolean;
  items: CheckItem[];
  metrics: { outlinePx: number; outlineColor: string; [key: string]: unknown };
}

/** Post-processing of one candidate (`post\cand-NN\checks.json`). */
export interface PostJobReport {
  ok: boolean;
  jobDir: string;
  candidate: number;
  kind: string;
  raw?: string;
  master: string;
  options: Record<string, unknown> & { outline: string; background: string };
  post: { alphaSource: string; removedParts: number; layout: { strokePx: number } };
  generated: { report: CheckReport; iou: number | null; modelOutline: boolean; nativeAlpha: boolean };
  sizes: { size: number; path: string; checks: CheckReport }[];
  /** strict or stylized ("" without a capture) */
  fidelityMode?: string;
  fidelity?: FidelityReport | null;
  reviews?: FidelityReview[];
  pass: boolean;
  elapsedMs: number;
}

export interface ExportReport {
  ok: boolean;
  folder: string;
  files: { size: number; path: string }[];
  error: string | null;
}

// ---------------------------------------------------------------- whole runs and jobs (pipeline.rs, jobs.rs)

/** A step of a run (uis-core pipeline.rs RunEvent). */
export interface RunEvent {
  runId: string;
  /** capture, prompt, rewrite, generate, post, check, export, done */
  stage: string;
  /** start, ok, fail, skip, cancel, progress */
  state: string;
  item: string | null;
  index: number;
  total: number;
  message: string;
  elapsedMs: number;
}

export interface RunCandidate {
  cand: number;
  ok: boolean;
  raw: string | null;
  failure: Failure | null;
  post: PostJobReport | null;
  postError: string | null;
  passed: number;
  checked: number;
  /** 1 for the first pictures, 2 and more for tries with corrections */
  attempt?: number;
  correction?: string | null;
}

/** One difference a fidelity review found (review.rs). */
export interface Finding {
  /** added, resized, missing or pattern */
  kind: string;
  what: string;
  detail: string;
  severity: string;
}

export interface FidelityReview {
  cli: string;
  ok: boolean;
  pass: boolean;
  failing: Finding[];
  notes: Finding[];
  summary: string;
  failure: Failure | null;
}

/** Fidelity of a picture to its capture (fidelity.rs). */
export interface FidelityReport {
  metrics: { profile: number; foreignBlobs: number; foreign: number; outsideParts: number; outside: number; missing: number };
  report: CheckReport;
  suspect: boolean;
  captureColor: [number, number];
  generatedColor: [number, number];
  map: string | null;
}

export interface RunItem {
  asset: string;
  name: string;
  kind: string;
  capture: string | null;
  jobDir: string | null;
  /** ok, failed, cancelled, stopped */
  state: string;
  stage: string | null;
  error: string | null;
  rewrite: RewriteReport | null;
  candidates: RunCandidate[];
  best: number | null;
  pass: boolean;
  outlineKept: boolean;
  export: ExportReport | null;
  elapsedMs: number;
}

export interface RunReport {
  schema: string;
  runId: string;
  project: string;
  ok: boolean;
  cancelled: boolean;
  stopped: string | null;
  error: string | null;
  capture: { runId: string; mode: string; reason: string; ok: boolean; error: string | null; failed: number; pending: boolean; elapsedMs: number; lineEndings?: string[]; left?: string[] } | null;
  items: RunItem[];
  elapsedMs: number;
}

/** What the screen sends to start a run (src-tauri run_start). */
export interface RunArgs {
  id: string;
  /** strict (keep the capture, add nothing) or stylized */
  fidelity?: string;
  items: { asset: string; kind?: string; capture?: string }[];
  note: string;
  cli: string;
  candidates: number;
  outline: string;
  background: string;
  outlineOffOk: boolean;
  noRefs: boolean;
}

/** A job folder in the 「Runs」 list (jobs.rs list). */
export interface JobSummary {
  dir: string;
  /** YYYYMMDD-HHMMSS */
  at: string;
  name: string | null;
  kind: string | null;
  packId: string | null;
  note: string | null;
  project: string | null;
  projectName: string | null;
  asset: string | null;
  ok: boolean | null;
  state: string | null;
  pass: boolean | null;
  failedStage: string | null;
  error: string | null;
  candidates: number[];
  posted: number[];
  best: number | null;
  icon: string | null;
  exported: boolean;
}

/** One job folder opened again (src-tauri job_get). */
export interface JobDetail {
  dir: string;
  job: Record<string, unknown>;
  posts: PostJobReport[];
  diff: PromptDiff | null;
  capture: string | null;
  export: { folder: string; files: { size: number; path: string }[]; at: string } | null;
}

/** The fixed prompt of a pack for a kind (prompt.rs Assembled). */
export interface Assembled {
  packId: string;
  templateVersion: string;
  sentences: Sentence[];
  text: string;
  addable: string[];
}

/** What a person changed for one prefab (projects.rs Override). */
export interface CameraOverride {
  kind?: string;
  yawSign?: number;
  rotation?: [number, number, number];
  front?: string;
  yaw?: number;
  pitch?: number;
  projection?: string;
  fov?: number;
}

/** The camera of one prefab (src-tauri prefab_camera): the kind's preset, with its rule, a
 * person's values and what a capture uses now. */
export interface CameraView {
  preset: Record<string, unknown>;
  rule: Record<string, unknown>;
  person: CameraOverride | null;
  effective: Record<string, unknown>;
}
