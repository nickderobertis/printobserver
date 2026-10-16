// Generated git histories the classifier is driven over, and the classifier
// itself, run as the workflows run it: `node .github/macos-policy.mjs`.
//
// Each history is a fresh repository holding this tree's own release-relevant
// files — release-plz.toml, the workspace and crate manifests, the lockfile, a
// changelog and a source file — as the base commit, and one commit on top that
// changes them the way the named diff class does. The version bump is made the
// way release-plz makes it: the workspace version, every path dependency's
// requirement, and the workspace's own packages in the lockfile.

import { execFileSync, spawnSync } from "node:child_process";
import {
  cpSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const GITHUB = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
export const ROOT = join(GITHUB, "..");
export const POLICY = join(GITHUB, "macos-policy.mjs");
export const FIXTURES = join(GITHUB, "tests", "fixtures", "events");

/** Every history this process made, removed when it exits however it exits. */
const made = [];
process.on("exit", () => {
  for (const dir of made) {
    rmSync(dir, { recursive: true, force: true });
  }
});

/** The files a history copies out of this tree as its base commit. */
export function baseFiles() {
  const crates = readdirSync(join(ROOT, "crates")).map((name) => `crates/${name}/Cargo.toml`);
  return [
    "release-plz.toml",
    "Cargo.toml",
    "Cargo.lock",
    ...crates,
    "crates/printobserver-core/CHANGELOG.md",
    "crates/printobserver-core/src/lib.rs",
  ];
}

/**
 * The variables naming a repository a git subprocess would otherwise operate
 * on, as `repo_checks.shell.GIT_LOCATION_VARIABLES` lists them; a test holds
 * the two lists to each other.
 */
export const GIT_LOCATION_VARIABLES = [
  "GIT_DIR",
  "GIT_WORK_TREE",
  "GIT_COMMON_DIR",
  "GIT_INDEX_FILE",
  "GIT_OBJECT_DIRECTORY",
  "GIT_ALTERNATE_OBJECT_DIRECTORIES",
  "GIT_NAMESPACE",
  "GIT_PREFIX",
  "GIT_QUARANTINE_PATH",
];

/**
 * The environment a child process gets: this one, minus anything naming a
 * repository, and minus what the forge or a budget check hands the process
 * running these tests — a gate on a pull request runs them under
 * `GITHUB_EVENT_NAME=pull_request`, which a test of the classifier with the
 * variable unset would otherwise inherit. `extra` is set on top.
 *
 * @param {Record<string, string>} [extra]
 * @returns {Record<string, string | undefined>}
 */
export function cleanEnvironment(extra = {}) {
  const environment = { ...process.env };
  for (const name of Object.keys(environment)) {
    if (
      GIT_LOCATION_VARIABLES.includes(name) ||
      /^GITHUB_(EVENT_NAME|EVENT_PATH|OUTPUT|STEP_SUMMARY|HEAD_REF|BASE_REF)$/.test(name) ||
      name.startsWith("ONEBUDGETSPEC_")
    ) {
      delete environment[name];
    }
  }
  return { ...environment, ...extra };
}

/**
 * @param {string[]} args
 * @param {string} cwd
 * @returns {string}
 */
export function git(args, cwd) {
  return execFileSync("git", args, {
    cwd,
    encoding: "utf8",
    env: cleanEnvironment({
      GIT_AUTHOR_NAME: "release-plz",
      GIT_AUTHOR_EMAIL: "release-plz@example.invalid",
      GIT_COMMITTER_NAME: "release-plz",
      GIT_COMMITTER_EMAIL: "release-plz@example.invalid",
    }),
  }).trim();
}

/** The workspace's own version, as the base Cargo.toml declares it. */
export function workspaceVersion() {
  const manifest = readFileSync(join(ROOT, "Cargo.toml"), "utf8");
  const found = /\[workspace\.package\][^[]*?\nversion = "([^"]+)"/.exec(manifest);
  if (!found) {
    throw new Error("Cargo.toml declares no [workspace.package] version");
  }
  return found[1];
}

/** The next patch version after `version`. */
export function bumped(version) {
  const [major, minor, patch] = version.split(".").map(Number);
  return `${major}.${minor}.${patch + 1}`;
}

/** How each diff class changes a base tree, by the files it rewrites. */
export const DIFFS = {
  source(dir) {
    append(dir, "crates/printobserver-core/src/lib.rs", "\n// A source change.\n");
  },
  version(dir) {
    bumpVersions(dir);
  },
  changelog(dir) {
    prependChangelog(dir);
  },
  dependency(dir) {
    // An external requirement moved alongside the version bump: what a release
    // pull request never carries, however much else of it looks like one.
    bumpVersions(dir);
    rewrite(dir, "Cargo.toml", (text) => {
      const moved = text.replace(/^serde_json = "1"$/m, 'serde_json = "1.0.999"');
      if (moved === text) {
        throw new Error('Cargo.toml carries no `serde_json = "1"` requirement to move');
      }
      return moved;
    });
  },
  mixed(dir) {
    bumpVersions(dir);
    append(dir, "crates/printobserver-core/src/lib.rs", "\n// Smuggled beside a release.\n");
  },
  "lockfile-source": (dir) => {
    // A lockfile entry that is not the workspace's own moved: a dependency
    // upgrade, however small, is not a version-only change.
    bumpVersions(dir);
    rewrite(dir, "Cargo.lock", (text) =>
      text.replace(/(name = "serde_json"\nversion = ")([^"]+)(")/, "$1$2-moved$3"),
    );
  },
  "version-garbled": (dir) => {
    rewrite(dir, "Cargo.toml", (text) =>
      text.replace(/(\[workspace\.package\][^[]*?\nversion = ")[^"]+(")/, "$1not-a-version$2"),
    );
  },
};

/**
 * How a diff class shapes the base commit before the change is made on top,
 * where the change needs something this tree does not carry.
 */
export const BASES = {
  "crate-path-version": (dir) => {
    const from = workspaceVersion();
    rewrite(
      dir,
      "crates/printobserver-server/Cargo.toml",
      (text) =>
        text.replace(
          "[dependencies]\n",
          `[dependencies]\nlocal-core = { path = "../printobserver-core", version = "${from}" }\n`,
        ) +
        `\n[target.'cfg(unix)'.dependencies]\nlocal-types = { path = "../printobserver-types", version = "${from}" }\n`,
    );
  },
  "lock-qualified": (dir) => {
    const from = workspaceVersion();
    rewrite(dir, "Cargo.lock", (text) =>
      text.replace(
        /(name = "printobserver-server"\nversion = "[^"]+"\ndependencies = \[\n(?: "[^"]+",\n)*?) "printobserver-core",/,
        `$1 "printobserver-core ${from}",`,
      ),
    );
  },
  "lock-source-qualified": (dir) => {
    const from = workspaceVersion();
    rewrite(dir, "Cargo.lock", (text) =>
      text.replace(
        /(name = "printobserver-server"\nversion = "[^"]+"\ndependencies = \[\n(?: "[^"]+",\n)*?) "printobserver-core",/,
        `$1 "printobserver-core ${from} (registry+https://github.com/rust-lang/crates.io-index)",`,
      ),
    );
  },
  "config-garbled": (dir) => {
    writeFileSync(join(dir, "release-plz.toml"), "[workspace\npr_branch_prefix = ");
  },
  "config-no-prefix": (dir) => {
    writeFileSync(join(dir, "release-plz.toml"), "[workspace]\nrelease_always = true\n");
  },
};

Object.assign(DIFFS, {
  empty() {},
  "changelog-added": (dir) => {
    writeFileSync(
      join(dir, "crates/printobserver-types/CHANGELOG.md"),
      "# Changelog\n\n## [Unreleased]\n",
    );
  },
  "changelog-deleted": (dir) => {
    rmSync(join(dir, "crates/printobserver-core/CHANGELOG.md"));
  },
  "crate-path-version": (dir) => {
    bumpVersions(dir);
    const from = workspaceVersion();
    rewrite(dir, "crates/printobserver-server/Cargo.toml", (text) =>
      text.replaceAll(`version = "${from}" }`, `version = "${bumped(from)}" }`),
    );
  },
  "lock-qualified": (dir) => {
    bumpVersions(dir);
    const from = workspaceVersion();
    rewrite(dir, "Cargo.lock", (text) => {
      const moved = text.replace(
        `"printobserver-core ${from}",`,
        `"printobserver-core ${bumped(from)}",`,
      );
      if (moved === text) {
        throw new Error(
          "the base lockfile carries no version-qualified printobserver-core reference",
        );
      }
      return moved;
    });
  },
  "lock-source-qualified": (dir) => {
    bumpVersions(dir);
    const from = workspaceVersion();
    rewrite(dir, "Cargo.lock", (text) =>
      text.replace(
        `"printobserver-core ${from} (registry+`,
        `"printobserver-core ${bumped(from)} (registry+`,
      ),
    );
  },
  "lock-version-garbled": (dir) => {
    bumpVersions(dir);
    rewrite(dir, "Cargo.lock", (text) =>
      text.replace(/(name = "printobserver-core"\nversion = ")[^"]+(")/, "$1garbled$2"),
    );
  },
  "config-garbled": (dir) => bumpVersions(dir),
  "config-no-prefix": (dir) => bumpVersions(dir),
});

function bumpVersions(dir) {
  const from = workspaceVersion();
  const to = bumped(from);
  rewrite(dir, "Cargo.toml", (text) =>
    text
      .replace(/(\[workspace\.package\][^[]*?\nversion = ")[^"]+(")/, `$1${to}$2`)
      .replace(
        new RegExp(`(\\{ path = "crates/[^"]+", version = ")${literal(from)}(" \\})`, "g"),
        `$1${to}$2`,
      ),
  );
  rewrite(dir, "Cargo.lock", (text) =>
    text.replace(
      new RegExp(`(name = "printobserver[a-z-]*"\\nversion = ")${literal(from)}(")`, "g"),
      `$1${to}$2`,
    ),
  );
  prependChangelog(dir);
}

function prependChangelog(dir) {
  rewrite(dir, "crates/printobserver-core/CHANGELOG.md", (text) => {
    const at = text.indexOf("\n## ");
    const entry = `\n## [${bumped(workspaceVersion())}] - 2026-10-16\n\n### Fixed\n\n- a fix\n`;
    return at < 0 ? `${text}${entry}` : `${text.slice(0, at)}${entry}${text.slice(at)}`;
  });
}

function literal(text) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function append(dir, path, text) {
  rewrite(dir, path, (before) => `${before}${text}`);
}

function rewrite(dir, path, change) {
  const file = join(dir, path);
  writeFileSync(file, change(readFileSync(file, "utf8")));
}

/**
 * A fresh history: this tree's release files as the base, and one commit of
 * the named diff class on a branch above it.
 *
 * @param {string} diff one of `DIFFS`
 * @returns {{dir: string, base: string, head: string}}
 */
export function history(diff) {
  const change = DIFFS[diff];
  if (!change) {
    throw new Error(`no diff class ${diff}`);
  }
  const dir = mkdtempSync(join(tmpdir(), `macos-policy-${diff}-`));
  made.push(dir);
  for (const path of baseFiles()) {
    mkdirSync(dirname(join(dir, path)), { recursive: true });
    cpSync(join(ROOT, path), join(dir, path));
  }
  BASES[diff]?.(dir);
  git(["init", "-q", "-b", "main"], dir);
  git(["add", "-A"], dir);
  git(["commit", "-q", "-m", "base"], dir);
  const base = git(["rev-parse", "HEAD"], dir);
  git(["checkout", "-q", "-b", "change"], dir);
  change(dir);
  git(["add", "-A"], dir);
  git(["commit", "-q", "--allow-empty", "-m", `a ${diff} change`], dir);
  return { dir, base, head: git(["rev-parse", "HEAD"], dir) };
}

/**
 * One event fixture, with the history's commits put where the forge puts them.
 *
 * @param {string} name a file of `fixtures/events`, without `.json`
 * @param {{base?: string, head?: string}} commits
 * @returns {Record<string, any>}
 */
export function fixture(name, commits = {}) {
  const text = readFileSync(join(FIXTURES, `${name}.json`), "utf8")
    .replaceAll("BASE_SHA", commits.base ?? "0".repeat(40))
    .replaceAll("BEFORE_SHA", commits.base ?? "0".repeat(40))
    .replaceAll("HEAD_SHA", commits.head ?? "f".repeat(40));
  return JSON.parse(text);
}

/**
 * Run the policy's command line as a workflow step does, in `cwd`.
 *
 * @param {string[]} args
 * @param {{cwd?: string, env?: Record<string, string>, input?: string}} [options]
 * @returns {{status: number | null, stdout: string, stderr: string}}
 */
export function policy(args, options = {}) {
  const ran = spawnSync(process.execPath, [POLICY, ...args], {
    cwd: options.cwd ?? ROOT,
    encoding: "utf8",
    env: cleanEnvironment(options.env ?? {}),
    input: options.input,
  });
  return { status: ran.status, stdout: ran.stdout, stderr: ran.stderr };
}

/**
 * Classify one event over one history, as the `classify` job does.
 *
 * @param {string} eventName
 * @param {Record<string, any>} payload
 * @param {{dir: string, base: string, head: string}} where
 * @returns {{isReleaseOnly: boolean, macosRequired: boolean, reason: string}}
 */
export function classified(eventName, payload, where) {
  const eventPath = join(where.dir, `${eventName}.event.json`);
  writeFileSync(eventPath, JSON.stringify(payload));
  const ran = policy(
    ["classify", "--event", eventPath, "--base", where.base, "--head", where.head],
    { cwd: where.dir, env: { GITHUB_EVENT_NAME: eventName } },
  );
  if (ran.status !== 0) {
    throw new Error(`classify exited ${ran.status}: ${ran.stderr}`);
  }
  return JSON.parse(ran.stdout);
}

/**
 * The `name=value` lines `outputs` prints for a classification, as a map.
 *
 * @param {object} classification
 * @returns {Record<string, string>}
 */
export function outputsOf(classification) {
  const ran = policy(["outputs", "-"], { input: JSON.stringify(classification) });
  if (ran.status !== 0) {
    throw new Error(`outputs exited ${ran.status}: ${ran.stderr}`);
  }
  return Object.fromEntries(
    ran.stdout
      .trim()
      .split(/\r?\n/)
      .map((line) => [line.slice(0, line.indexOf("=")), line.slice(line.indexOf("=") + 1)]),
  );
}
