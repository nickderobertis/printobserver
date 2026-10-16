// The one release-only classifier: whether a pull request is release
// automation's version/changelog-only pull request, and so whether the macOS
// lane is owed on it.
//
// A pull request is release-only when BOTH hold, and only then:
//
//   * its head branch is this repository's own (not a fork's) and begins with
//     the `pr_branch_prefix` the BASE tree's `release-plz.toml` declares — read
//     off the base, so a pull request cannot move the prefix it is judged by;
//   * its diff against the merge base touches nothing but CHANGELOG.md files
//     and the version fields of `Cargo.toml`, `crates/*/Cargo.toml` and
//     `Cargo.lock`, which is proven by parsing both sides and comparing them
//     with those fields taken out rather than by reading the diff's lines.
//
// The author is never consulted: release-plz opens its pull request under a
// personal token, so its author is the same login a person's would be. A
// title is never consulted either, because anybody can type one.
//
// Unknown is never omission. A pull request whose event, refs, diff or files
// cannot be read is classified as requiring macOS. Every event that is not a
// pull request is classified as requiring none, whatever it carries.

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { parse as parseToml } from "smol-toml";
import { isObject, message } from "./util.mjs";

/** The event an ordinary pull request's run is started by. */
export const PULL_REQUEST = "pull_request";

/** The files a release pull request may change, beside which nothing may move. */
const CHANGELOG = /(^|\/)CHANGELOG\.md$/;
const MANIFEST = /^(Cargo\.toml|crates\/[^/]+\/Cargo\.toml)$/;
const LOCKFILE = /^Cargo\.lock$/;

/** The dependency tables a Cargo manifest may carry, at its top or under `target.<cfg>`. */
const DEPENDENCY_TABLES = ["dependencies", "dev-dependencies", "build-dependencies"];

/**
 * @typedef {{isReleaseOnly: boolean, macosRequired: boolean, reason: string}} Classification
 */

/** A classification that cannot be decided, which on a pull request owes macOS. */
class Undecidable extends Error {}

/**
 * Classify one run's event.
 *
 * @param {{eventPath: string, base: string, head: string, eventName?: string, cwd?: string}} given
 * @returns {Classification}
 */
export function classify(given) {
  const cwd = given.cwd ?? process.cwd();
  let payload;
  try {
    payload = JSON.parse(readFileSync(given.eventPath, "utf8"));
  } catch (unreadable) {
    if (given.eventName && given.eventName !== PULL_REQUEST) {
      return notAPullRequest(given.eventName);
    }
    return owed(`the event payload ${given.eventPath} could not be read: ${message(unreadable)}`);
  }
  const eventName = given.eventName || inferred(payload);
  if (eventName !== PULL_REQUEST) {
    return eventName ? notAPullRequest(eventName) : owed("the event is not one this can name");
  }
  try {
    return pullRequest(payload, given.base, given.head, cwd);
  } catch (undecided) {
    if (undecided instanceof Undecidable) {
      return owed(undecided.message);
    }
    return owed(`classification failed: ${message(undecided)}`);
  }
}

/**
 * @param {string} eventName
 * @returns {Classification}
 */
function notAPullRequest(eventName) {
  return {
    isReleaseOnly: false,
    macosRequired: false,
    reason: `a ${eventName} run is not a pull request, and no automatic run but a pull request's requests macOS`,
  };
}

/**
 * @param {string} why
 * @returns {Classification}
 */
function owed(why) {
  return { isReleaseOnly: false, macosRequired: true, reason: `macOS required: ${why}` };
}

/**
 * The event name a payload is shaped like, for a caller that did not say.
 *
 * @param {unknown} payload
 * @returns {string}
 */
function inferred(payload) {
  if (!isObject(payload)) {
    return "";
  }
  if (isObject(payload.pull_request)) {
    return PULL_REQUEST;
  }
  if (isObject(payload.workflow_run)) {
    return "workflow_run";
  }
  if (isObject(payload.release)) {
    return "release";
  }
  if ("schedule" in payload) {
    return "schedule";
  }
  if (isObject(payload.inputs) || "workflow" in payload) {
    return "workflow_dispatch";
  }
  if (typeof payload.ref === "string" && "before" in payload && "after" in payload) {
    return "push";
  }
  return "";
}

/**
 * @param {Record<string, unknown>} payload
 * @param {string} base
 * @param {string} head
 * @param {string} cwd
 * @returns {Classification}
 */
function pullRequest(payload, base, head, cwd) {
  const pr = payload.pull_request;
  if (!isObject(pr) || !isObject(pr.head) || !isObject(pr.base)) {
    throw new Undecidable("the pull request's head and base are not in its event");
  }
  const branch = pr.head.ref;
  const headRepo = isObject(pr.head.repo) ? pr.head.repo : undefined;
  const baseRepo = isObject(pr.base.repo) ? pr.base.repo : undefined;
  if (typeof branch !== "string" || !headRepo || !baseRepo) {
    throw new Undecidable("the pull request's head branch or repositories are not in its event");
  }
  if (!base || !head) {
    throw new Undecidable("no base or head ref was given to diff");
  }
  const sameRepository =
    typeof headRepo.full_name === "string" &&
    headRepo.full_name === baseRepo.full_name &&
    headRepo.fork !== true;
  const prefix = releaseBranchPrefix(base, cwd);
  if (!sameRepository || !branch.startsWith(prefix)) {
    const whose = sameRepository ? "this repository's" : "a fork's";
    return owed(
      `an ordinary pull request (${whose} branch \`${branch}\`, not one under \`${prefix}\` in this repository)`,
    );
  }
  const changed = changedFiles(base, head, cwd);
  if (changed.files.length === 0) {
    return owed(`release branch \`${branch}\` changes nothing, which no release does`);
  }
  for (const file of changed.files) {
    const refused = versionOnly(file, changed.mergeBase, head, cwd);
    if (refused) {
      return owed(`release branch \`${branch}\` ${refused}`);
    }
  }
  return {
    isReleaseOnly: true,
    macosRequired: false,
    reason: `release branch \`${branch}\` of this repository changes only version fields and changelogs (${changed.files
      .map((file) => file.path)
      .join(", ")})`,
  };
}

/**
 * The release-plz branch prefix the base tree declares.
 *
 * @param {string} base
 * @param {string} cwd
 * @returns {string}
 */
function releaseBranchPrefix(base, cwd) {
  const text = show(base, "release-plz.toml", cwd);
  let declared;
  try {
    declared = parseToml(text);
  } catch (malformed) {
    throw new Undecidable(`release-plz.toml at ${base} is not TOML: ${message(malformed)}`);
  }
  const workspace = declared.workspace;
  const prefix = isObject(workspace)
    ? /** @type {Record<string, unknown>} */ (workspace).pr_branch_prefix
    : undefined;
  if (typeof prefix !== "string" || !prefix) {
    throw new Undecidable(`release-plz.toml at ${base} names no [workspace] pr_branch_prefix`);
  }
  return prefix;
}

/**
 * @typedef {{status: string, path: string}} Changed
 */

/**
 * Every file the pull request changes against its merge base with `base`.
 *
 * @param {string} base
 * @param {string} head
 * @param {string} cwd
 * @returns {{mergeBase: string, files: Changed[]}}
 */
function changedFiles(base, head, cwd) {
  const mergeBase = git(["merge-base", base, head], cwd).trim();
  const listing = git(["diff", "--name-status", "--no-renames", "-z", mergeBase, head], cwd);
  const fields = listing.split("\0").filter((field) => field !== "");
  /** @type {Changed[]} */
  const files = [];
  for (let at = 0; at < fields.length; at += 2) {
    const status = fields[at];
    const path = fields[at + 1];
    if (status === undefined || path === undefined) {
      throw new Undecidable("git's list of changed files could not be read");
    }
    files.push({ status, path });
  }
  return { mergeBase, files };
}

/**
 * Why one changed file is not a version/changelog-only change, or "" when it is.
 *
 * @param {Changed} file
 * @param {string} mergeBase
 * @param {string} head
 * @param {string} cwd
 * @returns {string}
 */
function versionOnly(file, mergeBase, head, cwd) {
  if (CHANGELOG.test(file.path)) {
    return file.status === "M" || file.status === "A"
      ? ""
      : `${statusWord(file.status)} changelog ${file.path}`;
  }
  const manifest = MANIFEST.test(file.path);
  const lockfile = LOCKFILE.test(file.path);
  if (!manifest && !lockfile) {
    return `changes ${file.path}, which is neither a changelog nor a version manifest`;
  }
  if (file.status !== "M") {
    return `${statusWord(file.status)} ${file.path}, where a release only rewrites one`;
  }
  const before = parsed(mergeBase, file.path, cwd);
  const after = parsed(head, file.path, cwd);
  const strip = manifest ? withoutManifestVersions : withoutLockVersions;
  const members = manifest ? new Set() : workspaceMembers(before);
  const left = strip(before, members);
  const right = strip(after, members);
  if (!left.ok || !right.ok) {
    return `changes ${file.path} in a way that is not a version: ${left.why || right.why}`;
  }
  return equal(left.value, right.value)
    ? ""
    : `changes ${file.path} beyond its version fields (${difference(left.value, right.value)})`;
}

/**
 * @param {string} status
 * @returns {string}
 */
function statusWord(status) {
  return { A: "adds", D: "deletes", T: "retypes", M: "modifies" }[status] ?? "changes";
}

/**
 * @param {string} ref
 * @param {string} path
 * @param {string} cwd
 * @returns {Record<string, unknown>}
 */
function parsed(ref, path, cwd) {
  try {
    return parseToml(show(ref, path, cwd));
  } catch (malformed) {
    if (malformed instanceof Undecidable) {
      throw malformed;
    }
    throw new Undecidable(`${path} at ${ref} is not TOML: ${message(malformed)}`);
  }
}

/** A version field: dotted numbers and an optional pre-release or build. */
const VERSION = /^\d+\.\d+\.\d+([-+][0-9A-Za-z.-]+)?$/;

/**
 * @typedef {{ok: true, value: unknown, why?: undefined} | {ok: false, why: string, value?: undefined}} Stripped
 */

/**
 * A manifest with its version fields taken out: the workspace's and the
 * package's own, and the requirement of every dependency it names by path.
 *
 * @param {Record<string, unknown>} document
 * @returns {Stripped}
 */
function withoutManifestVersions(document) {
  const copy = structuredClone(document);
  const problems = [];
  const take = (table, key, where) => {
    if (!isObject(table) || !(key in table)) {
      return;
    }
    const value = table[key];
    if (isObject(value) && value.workspace === true) {
      return;
    }
    if (typeof value !== "string" || !VERSION.test(value)) {
      problems.push(`${where} is ${JSON.stringify(value)}, not a version`);
      return;
    }
    delete table[key];
  };
  if (isObject(copy.workspace)) {
    take(copy.workspace.package, "version", "workspace.package.version");
    stripPathDependencies(copy.workspace.dependencies, "workspace.dependencies", take);
  }
  take(copy.package, "version", "package.version");
  for (const table of DEPENDENCY_TABLES) {
    stripPathDependencies(copy[table], table, take);
  }
  if (isObject(copy.target)) {
    for (const [cfg, scoped] of Object.entries(copy.target)) {
      for (const table of DEPENDENCY_TABLES) {
        stripPathDependencies(
          isObject(scoped) ? scoped[table] : undefined,
          `target.${cfg}.${table}`,
          take,
        );
      }
    }
  }
  return problems.length ? { ok: false, why: problems.join("; ") } : { ok: true, value: copy };
}

/**
 * @param {unknown} table
 * @param {string} where
 * @param {(table: unknown, key: string, where: string) => void} take
 */
function stripPathDependencies(table, where, take) {
  if (!isObject(table)) {
    return;
  }
  for (const [name, requirement] of Object.entries(table)) {
    if (isObject(requirement) && typeof requirement.path === "string") {
      take(requirement, "version", `${where}.${name}.version`);
    }
  }
}

/**
 * The names of the workspace's own packages: those the lockfile records with no source.
 *
 * @param {Record<string, unknown>} lock
 * @returns {Set<string>}
 */
function workspaceMembers(lock) {
  const packages = Array.isArray(lock.package) ? lock.package : [];
  return new Set(
    packages
      .filter((entry) => isObject(entry) && !("source" in entry) && typeof entry.name === "string")
      .map((entry) => entry.name),
  );
}

/**
 * A lockfile with the workspace's own packages' versions taken out, wherever
 * they are recorded: on the package itself and in a dependency naming it.
 *
 * @param {Record<string, unknown>} lock
 * @param {Set<string>} members
 * @returns {Stripped}
 */
function withoutLockVersions(lock, members) {
  const copy = structuredClone(lock);
  const packages = Array.isArray(copy.package) ? copy.package : [];
  const problems = [];
  for (const entry of packages) {
    if (!isObject(entry)) {
      continue;
    }
    if (members.has(entry.name) && !("source" in entry)) {
      if (typeof entry.version !== "string" || !VERSION.test(entry.version)) {
        problems.push(`${entry.name}'s version is ${JSON.stringify(entry.version)}`);
      }
      delete entry.version;
    }
    if (Array.isArray(entry.dependencies)) {
      entry.dependencies = entry.dependencies.map((dependency) => {
        const [name, version] = String(dependency).split(" ");
        return members.has(name) && version && VERSION.test(version) ? name : dependency;
      });
    }
  }
  return problems.length ? { ok: false, why: problems.join("; ") } : { ok: true, value: copy };
}

/**
 * @param {unknown} left
 * @param {unknown} right
 * @returns {boolean}
 */
function equal(left, right) {
  return difference(left, right) === "";
}

/**
 * The first path at which two parsed documents differ, or "" where they do not.
 *
 * @param {unknown} left
 * @param {unknown} right
 * @param {string} [at]
 * @returns {string}
 */
function difference(left, right, at = "") {
  if (Array.isArray(left) || Array.isArray(right)) {
    if (!Array.isArray(left) || !Array.isArray(right) || left.length !== right.length) {
      return at || "(document)";
    }
    for (let index = 0; index < left.length; index += 1) {
      const found = difference(left[index], right[index], `${at}[${index}]`);
      if (found) {
        return found;
      }
    }
    return "";
  }
  if (isObject(left) && isObject(right)) {
    const keys = new Set([...Object.keys(left), ...Object.keys(right)]);
    for (const key of [...keys].sort()) {
      const found = difference(left[key], right[key], at ? `${at}.${key}` : key);
      if (found) {
        return found;
      }
    }
    return "";
  }
  if (left instanceof Date || right instanceof Date) {
    return String(left) === String(right) ? "" : at;
  }
  return Object.is(left, right) ? "" : at || "(document)";
}

/**
 * @param {string} ref
 * @param {string} path
 * @param {string} cwd
 * @returns {string}
 */
function show(ref, path, cwd) {
  return git(["show", `${ref}:${path}`], cwd);
}

/**
 * @param {string[]} args
 * @param {string} cwd
 * @returns {string}
 */
function git(args, cwd) {
  try {
    return execFileSync("git", args, {
      cwd,
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
      stdio: ["ignore", "pipe", "pipe"],
    });
  } catch (failed) {
    const said = isObject(failed) && failed.stderr ? String(failed.stderr).trim() : message(failed);
    throw new Undecidable(`\`git ${args.join(" ")}\` failed: ${said}`);
  }
}
