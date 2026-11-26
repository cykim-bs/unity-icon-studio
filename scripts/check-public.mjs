// Checks the files of this repository before they are pushed to a public place.
//
//   node scripts/check-public.mjs [--denylist <file>]... [--allow <file>]... [--binaries <file>] [--root <dir>]
//
// Files: what git tracks or would track (tracked + untracked, ignored ones left out).
// Each path and each text line is checked against:
//   - every --denylist file: one regular expression per line (case-insensitive), "#" starts a
//     comment line. Keep these lists outside the repository: they name what must not appear;
//   - built-in rules: e-mail addresses other than GitHub noreply ones, user-profile paths
//     (Windows and posix), and drive paths other than C:.
// Before the rules run on a line, spans an --allow file matches (one regular expression per line,
// case-insensitive) are blanked, and so is the account name inside a GitHub URL
// (https://github.com/<account>/...): a public link is not an internal name. Keep allow patterns
// to the one token that may pass, so the rest of the line is still checked.
// Binary files (a NUL byte in the first 8000 bytes) cannot be read by rules: each is printed with
// its SHA-256. With --binaries <file> ("<sha256> <path>" per line, written after a person looked
// at the file) a binary that is not listed, or changed since, is a finding.
// Exit 0 = clean, 4 = findings (listed), 1 = error.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const USAGE = "usage: node scripts/check-public.mjs [--denylist <file>]... [--allow <file>]... [--binaries <file>] [--root <dir>]";

function parseArgs(argv) {
  const out = { denylists: [], allows: [], binaries: null, root: join(dirname(fileURLToPath(import.meta.url)), "..") };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const next = () => {
      if (i + 1 >= argv.length) throw new Error(`${a} needs a value`);
      return argv[++i];
    };
    if (a === "--denylist") out.denylists.push(next());
    else if (a === "--allow") out.allows.push(next());
    else if (a === "--binaries") out.binaries = next();
    else if (a === "--root") out.root = next();
    else if (a === "--help" || a === "-h") {
      console.log(USAGE);
      process.exit(0);
    } else throw new Error(`unknown argument ${a}\n${USAGE}`);
  }
  return out;
}

export const BUILTIN_RULES = [
  // the noreply addresses GitHub gives are fine; any other address is not
  { name: "e-mail address", regex: /[A-Za-z0-9._%+-]+@(?!users\.noreply\.github\.com\b)[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}/i },
  { name: "user profile path", regex: /[A-Za-z]:[\\/]+Users[\\/]+[^\\/\s"'`]+/i },
  { name: "user profile path (posix)", regex: /(^|[\s"'`(])\/(home|Users)\/[^/\s"'`]+/ },
  { name: "drive other than C:", regex: /(?<![A-Za-z])[D-Zd-z]:[\\/]/ },
];

/** The account name in a GitHub URL. */
export const BUILTIN_ALLOW = [/(?<=https:\/\/github\.com\/)[A-Za-z0-9-]+(?=\/)/gi];

function regexLines(file, what) {
  if (!existsSync(file)) throw new Error(`no such ${what}: ${file}`);
  return readFileSync(file, "utf8")
    .replace(/^﻿/, "")
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l && !l.startsWith("#"));
}

export function loadRules(files) {
  const rules = files.flatMap((file) => regexLines(file, "denylist").map((line) => ({ name: line, regex: new RegExp(line, "i") })));
  return [...rules, ...BUILTIN_RULES];
}

export function loadAllows(files) {
  return [...files.flatMap((file) => regexLines(file, "allow list").map((line) => new RegExp(line, "gi"))), ...BUILTIN_ALLOW];
}

/** `line` with the allowed spans blanked (same length, so columns stay). */
export function mask(line, allows) {
  let out = line;
  for (const a of allows) out = out.replace(a, (m) => " ".repeat(m.length));
  return out;
}

export function isBinary(bytes) {
  const n = Math.min(bytes.length, 8000);
  for (let i = 0; i < n; i++) if (bytes[i] === 0) return true;
  return false;
}

/** Findings for one file (as text lines), and the SHA-256 of a binary file. */
export function checkFile(rel, bytes, rules, allowed, allows = BUILTIN_ALLOW) {
  const found = [];
  for (const r of rules) if (r.regex.test(rel)) found.push(`${rel}: path matches [${r.name}]`);
  if (isBinary(bytes)) {
    const hash = createHash("sha256").update(bytes).digest("hex");
    if (allowed) {
      const want = allowed.get(rel);
      if (want !== hash) found.push(`${rel}: binary file ${want ? "changed since it was listed" : "not listed"}. Look at it; if it is fine, add: ${hash} ${rel}`);
    }
    return { found, binary: hash };
  }
  const lines = new TextDecoder("utf-8").decode(bytes).split(/\r?\n/);
  lines.forEach((line, i) => {
    const checked = mask(line, allows);
    for (const r of rules) {
      const m = r.regex.exec(checked);
      if (!m) continue;
      const start = Math.max(0, m.index - 30);
      found.push(`${rel}:${i + 1}: [${r.name}] ${line.slice(start, start + 100).trim()}`);
    }
  });
  return { found, binary: null };
}

function gitFiles(root) {
  const out = execFileSync("git", ["-C", root, "ls-files", "--cached", "--others", "--exclude-standard", "-z"], { encoding: "utf8" });
  return [...new Set(out.split("\0").filter(Boolean))].filter((rel) => existsSync(join(root, rel))).sort();
}

function loadAllowed(file) {
  const map = new Map();
  for (const line of regexLines(file, "binary list")) {
    const m = /^([0-9a-f]{64})\s+(.+)$/i.exec(line);
    if (!m) throw new Error(`${file}: not "<sha256> <path>": ${line}`);
    map.set(m[2], m[1].toLowerCase());
  }
  return map;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const root = resolve(args.root);
  const rules = loadRules(args.denylists);
  const allows = loadAllows(args.allows);
  const allowed = args.binaries ? loadAllowed(args.binaries) : null;
  const files = gitFiles(root);
  const findings = [];
  const binaries = [];
  for (const rel of files) {
    const { found, binary } = checkFile(rel, readFileSync(join(root, rel)), rules, allowed, allows);
    findings.push(...found);
    if (binary) binaries.push(`${binary} ${rel}`);
  }
  console.log(
    `files   ${files.length}, rules ${rules.length} (${rules.length - BUILTIN_RULES.length} from ${args.denylists.length} denylist file(s) + ${BUILTIN_RULES.length} built-in), ${allows.length} allow pattern(s)`,
  );
  if (binaries.length) {
    console.log(`binary  ${binaries.length} file(s)${allowed ? "" : " (look at each one; --binaries <file> makes this a check)"}:`);
    for (const b of binaries) console.log(`  ${b}`);
  }
  if (findings.length) {
    console.log(`FOUND   ${findings.length}:`);
    for (const f of findings) console.log(`  ${f}`);
    process.exit(4);
  }
  console.log("clean   0 findings");
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (e) {
    console.error(`check-public: ${e instanceof Error ? e.message : e}`);
    process.exit(1);
  }
}
