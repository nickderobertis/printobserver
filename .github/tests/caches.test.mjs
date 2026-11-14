// Every cache `ci.yml` keeps is restored by every run and saved by a run on
// `main` alone.
//
// GitHub scopes a cache saved by a pull request's run to that pull request's
// merge ref, so no other run restores it. The repository's quota is one 10 GB
// pool, so open pull requests saving their caches evict `main`'s, which are the
// only caches every pull request restores, and every pull request after them
// runs cold. So each cache step of the committed workflow is read here as GitHub
// runs it, for each matrix cell of its job, under two runs: a push to `main`, and
// a pull request. Its conditions and inputs are evaluated by `graph.mjs`, over
// GitHub's own published expression library.
//
// * Under both runs it restores, and it looks up the cache the push to `main`
//   saves: every input its key and restore keys are made of renders the same
//   under both. A key that reads the ref, the event or the pull request cannot.
// * Under the push to `main` it saves when its key missed. Under the pull
//   request it saves nothing, hit or miss.
//
// What decides whether an action caches and saves is read from the inputs its
// step sets. This test never assumes an input's default: a caching action that
// leaves its save input unset is refused, whatever the action does without it.
// An action whose restore and save cannot be told apart, `actions/cache` itself
// and Bun's cache of its own executable, must not cache at all. An action this
// module does not know is refused rather than assumed to keep nothing.
//
// One cache is exempt, and it is named: the real-print suite's LFS store, keyed
// on the cases' content and saved only when that key missed. Only a pull request
// changing the cases saves one (about 150 MB). Saving it from `main` alone would
// download the cases again on every push of that pull request, out of the
// account's monthly LFS bandwidth.

import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, test } from "node:test";
import { condition, readWorkflow, template } from "../lib/graph.mjs";
import { isObject } from "../lib/util.mjs";
import { WORKFLOWS } from "./support/lifecycle.mjs";

const CI = join(WORKFLOWS, "ci.yml");

/** The two runs a cache step is read under, by the `github` context each hands it. */
const RUNS = {
  main: {
    label: "push to `main`",
    github: { ref: "refs/heads/main", event_name: "push", base_ref: "" },
  },
  pullRequest: {
    label: "pull request",
    github: { ref: "refs/pull/328/merge", event_name: "pull_request", base_ref: "main" },
  },
};

/** The actions that keep no cache whatever their inputs. */
// llmlint: ignore[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
const CACHELESS = new Set([
  "actions/checkout",
  "actions/upload-artifact",
  "extractions/setup-just",
  "nrwl/nx-set-shas",
  "taiki-e/install-action",
]);

/**
 * How one caching action is told what to keep, by the inputs its step sets.
 *
 * @typedef {object} Caching
 * @property {string | null} onlyWith the input without which it keeps no cache, if it caches only where asked
 * @property {{input: string, value: string} | null} off the input that turns its cache off and the value that does, if one does
 * @property {string | null} restore the input that turns its restore off, if one does
 * @property {string | null} save the input deciding whether it saves, or `null` where it restores and saves as one
 * @property {string[]} keyedBy the inputs its key and restore keys are made of, beside what it hashes itself
 */

/** @type {Record<string, Caching>} */
// llmlint: ignore[contracts_have_one_source_or_a_drift_gate] suppressions.toml has the reason.
const CACHING = {
  "actions-rust-lang/setup-rust-toolchain": {
    onlyWith: null,
    off: { input: "cache", value: "false" },
    restore: null,
    save: "cache-save-if",
    keyedBy: ["cache-key", "cache-shared-key", "cache-workspaces", "cache-directories"],
  },
  "astral-sh/setup-uv": {
    onlyWith: null,
    off: { input: "enable-cache", value: "false" },
    restore: "restore-cache",
    save: "save-cache",
    keyedBy: ["cache-suffix", "cache-dependency-glob", "cache-local-path"],
  },
  "oven-sh/setup-bun": {
    onlyWith: null,
    off: { input: "no-cache", value: "true" },
    restore: null,
    save: null,
    keyedBy: [],
  },
  "actions/cache": { onlyWith: null, off: null, restore: null, save: null, keyedBy: [] },
  "actions/setup-node": { onlyWith: "cache", off: null, restore: null, save: null, keyedBy: [] },
};
const RESTORE = "actions/cache/restore";
const SAVE = "actions/cache/save";
/** The one cache a pull request may save, by the path it holds. */
const EXEMPT = ".git/lfs/objects";

/** One matrix cell of one job, and the steps it runs. */
class Cell {
  /**
   * @param {string} job
   * @param {Record<string, any>} values
   * @param {Record<string, any>[]} steps
   */
  constructor(job, values, steps) {
    this.job = job;
    this.values = values;
    this.steps = steps;
  }

  toString() {
    const named = ["id", "shard"]
      .filter((key) => key in this.values)
      .map((key) => this.values[key]);
    return `job \`${this.job}\`${named.length ? ` (${named.join(", ")})` : ""}`;
  }

  /**
   * The contexts a step of this cell is evaluated in, under one run. Every
   * step output the job reads stands for itself, so it renders the same on
   * both runs; `hits` overrides a restore step's `cache-hit`.
   *
   * @param {{github: Record<string, string>}} run
   * @param {Record<string, string>} [hits]
   */
  contexts(run, hits = {}) {
    /** @type {Record<string, {outputs: Record<string, string>}>} */
    const steps = {};
    for (const [, id, name] of JSON.stringify(this.steps).matchAll(
      /steps\.([\w-]+)\.outputs\.([\w-]+)/g,
    )) {
      steps[id] ??= { outputs: {} };
      steps[id].outputs[name] = `<${id}.${name}>`;
    }
    for (const [id, hit] of Object.entries(hits)) {
      steps[id] ??= { outputs: {} };
      steps[id].outputs["cache-hit"] = hit;
    }
    return { github: run.github, matrix: { platform: this.values }, steps };
  }

  /**
   * An input as the step hands it to its action under one run, as the string the action reads.
   *
   * @param {Record<string, any>} step
   * @param {string} input
   * @param {{github: Record<string, string>}} run
   * @param {Record<string, string>} [hits]
   */
  input(step, input, run, hits) {
    return String(template(String(step.with[input]), this.contexts(run, hits), `${this}`));
  }

  /**
   * Whether the step runs under one run.
   *
   * @param {Record<string, any>} step
   * @param {{github: Record<string, string>}} run
   * @param {Record<string, string>} [hits]
   */
  runs(step, run, hits) {
    return condition(step.if, this.contexts(run, hits), [], `${this}`);
  }
}

/**
 * Each cell a workflow's jobs run as: one per `platform` matrix entry, or the job alone.
 *
 * @param {{jobs: Record<string, any>}} workflow
 */
function cells(workflow) {
  return Object.entries(workflow.jobs).flatMap(([name, job]) => {
    const steps = (job.steps ?? []).map((/** @type {Record<string, any>} */ step) => ({
      ...step,
      with: isObject(step.with) ? step.with : {},
    }));
    const entries = job.strategy?.matrix?.platform;
    return Array.isArray(entries)
      ? entries.map((entry) => new Cell(name, entry, steps))
      : [new Cell(name, {}, steps)];
  });
}

/** @param {Record<string, any>} step */
function action(step) {
  return String(step.uses ?? "").split("@")[0];
}

/**
 * Every way a cache step of `workflow` is not restored everywhere and saved on `main` alone.
 *
 * @param {{jobs: Record<string, any>}} workflow
 * @returns {string[]}
 */
function findings(workflow) {
  return cells(workflow).flatMap((cell) => cellFindings(cell));
}

/** @param {Cell} cell */
function cellFindings(cell) {
  /** @type {string[]} */
  const found = [];
  /** @type {Map<string, string>} the key each restore step looks up on `main`, by its id */
  const restored = new Map();
  cell.steps.forEach((step, index) => {
    const used = action(step);
    const where = `${cell} step ${index + 1} (\`${step.uses}\`)`;
    const caching = CACHING[used];
    if (!used || CACHELESS.has(used)) {
      return;
    }
    if (used === RESTORE) {
      found.push(...restoreFindings(cell, step, where));
      restored.set(String(step.id), cell.input(step, "key", RUNS.main));
    } else if (used === SAVE) {
      found.push(...saveFindings(cell, step, where, restored));
    } else if (caching === undefined) {
      found.push(`${where} uses an action this test does not know whether it caches`);
    } else {
      found.push(...cachingFindings(cell, step, where, caching));
    }
  });
  return found;
}

/**
 * @param {Cell} cell
 * @param {Record<string, any>} step
 * @param {string} where
 * @param {Caching} caching
 */
function cachingFindings(cell, step, where, caching) {
  const { onlyWith, off, restore, save, keyedBy } = caching;
  if (onlyWith !== null && !(onlyWith in step.with)) {
    return [];
  }
  if (off && off.input in step.with && cell.input(step, off.input, RUNS.main) === off.value) {
    return [];
  }
  if (save === null) {
    return [
      `${where} caches, and restores and saves as one, so it cannot restore on a pull request without saving there${off ? `: set \`${off.input}: ${off.value}\`` : ""}`,
    ];
  }
  if (!(save in step.with)) {
    return [
      `${where} leaves \`${save}\` unset, so whether it saves on a pull request's ref is the action's default`,
    ];
  }
  const found = [];
  for (const run of Object.values(RUNS)) {
    if (restore !== null && restore in step.with && cell.input(step, restore, run) === "false") {
      found.push(
        `${where} restores nothing on a ${run.label}: its \`${restore}\` renders \`false\``,
      );
    }
  }
  if (cell.input(step, save, RUNS.pullRequest) !== "false") {
    found.push(`${where} saves on a pull request's ref: its \`${save}\` does not render \`false\``);
  }
  if (cell.input(step, save, RUNS.main) !== "true") {
    found.push(
      `${where} does not save on a push to \`main\`: its \`${save}\` does not render \`true\``,
    );
  }
  for (const input of keyedBy.filter((one) => one in step.with)) {
    if (cell.input(step, input, RUNS.main) !== cell.input(step, input, RUNS.pullRequest)) {
      found.push(
        `${where}'s \`${input}\` renders differently on a pull request, so its lookup cannot match the cache a push to \`main\` saves`,
      );
    }
  }
  return found;
}

/**
 * A restore runs on every run and looks up what a push to `main` saves.
 *
 * @param {Cell} cell
 * @param {Record<string, any>} step
 * @param {string} where
 */
function restoreFindings(cell, step, where) {
  const found = [];
  for (const run of Object.values(RUNS)) {
    if (!cell.runs(step, run)) {
      found.push(`${where} restores nothing on a ${run.label}: its condition skips it`);
    }
  }
  for (const input of ["key", "restore-keys"].filter((one) => one in step.with)) {
    if (cell.input(step, input, RUNS.main) !== cell.input(step, input, RUNS.pullRequest)) {
      found.push(
        `${where}'s \`${input}\` renders differently on a pull request, so its lookup cannot match the cache a push to \`main\` saves`,
      );
    }
  }
  return found;
}

/**
 * A save follows its restore, under its key, and runs on `main` alone.
 *
 * @param {Cell} cell
 * @param {Record<string, any>} step
 * @param {string} where
 * @param {Map<string, string>} restored
 */
function saveFindings(cell, step, where, restored) {
  const key = cell.input(step, "key", RUNS.main);
  const pair = [...restored].find(([, looked]) => looked === key)?.[0];
  if (pair === undefined) {
    return [`${where} saves under a key no restore step before it looks up`];
  }
  const found = [];
  if (!cell.runs(step, RUNS.main, { [pair]: "false" })) {
    found.push(`${where} does not save on a push to \`main\` whose lookup missed`);
  }
  if (String(step.with.path) === EXEMPT) {
    return found;
  }
  for (const hit of ["true", "false"]) {
    if (cell.runs(step, RUNS.pullRequest, { [pair]: hit })) {
      found.push(`${where} saves on a pull request's ref (cache-hit ${hit})`);
    }
  }
  return found;
}

/** The committed workflow's text. */
const TEXT = readFileSync(CI, "utf8");

/**
 * The findings over the committed workflow with one passage replaced.
 *
 * @param {string} old
 * @param {string} replacement
 */
function brokenFindings(old, replacement) {
  assert.ok(TEXT.includes(old), `the committed workflow carries ${JSON.stringify(old)}`);
  const directory = mkdtempSync(join(tmpdir(), "caches-"));
  writeFileSync(join(directory, "ci.yml"), TEXT.replace(old, replacement));
  return findings(readWorkflow(directory, "ci.yml")).join("\n");
}

/** An Actions expression as a workflow writes it, without spelling its opener in a literal. */
function expression(inner) {
  return `${"$"}{{ ${inner} }}`;
}

const RELEASE_KEY = `          key: release-program-${expression("matrix.platform.id")}-${expression("steps.release-program.outputs.key")}\n`;
const RELEASE_SAVE_IF =
  "      - if: github.ref == 'refs/heads/main' && steps.release-program-cache.outputs.cache-hit != 'true'\n";
const SHARD_KEY = `          cache-key: shard-${expression("matrix.platform.shard")}-of-${expression("matrix.platform.shards")}\n`;
const RUST_SAVE_IF = `          cache-save-if: ${expression("github.ref == 'refs/heads/main'")}\n`;
const UV_SAVE = `          save-cache: ${expression("github.ref == 'refs/heads/main'")}\n`;

describe("the committed ci.yml's caches", () => {
  test("every cache is restored on every run and saved on main alone", () => {
    assert.deepEqual(findings(readWorkflow(WORKFLOWS, "ci.yml")), []);
  });

  test("it keeps every cache it claims to, so the walk over it reads something", () => {
    const steps = cells(readWorkflow(WORKFLOWS, "ci.yml")).flatMap((cell) => cell.steps);
    const used = new Set(steps.map(action));
    for (const kept of [
      "actions-rust-lang/setup-rust-toolchain",
      "astral-sh/setup-uv",
      RESTORE,
      SAVE,
    ]) {
      assert.ok(used.has(kept), `a step uses ${kept}`);
    }
    for (const step of steps.filter((one) => action(one) === "oven-sh/setup-bun")) {
      assert.equal(step.with["no-cache"], true, "a Bun step's `no-cache`");
    }
    assert.ok(
      steps.some((step) => action(step) === SAVE && step.with.path === EXEMPT),
      "the real-print store's save step",
    );
  });
});

describe("a cache saved on a pull request's ref is refused", () => {
  const cases = [
    ["the Rust cache's save input left unset", RUST_SAVE_IF, "", "leaves `cache-save-if` unset"],
    [
      "the Rust cache saved on any push",
      RUST_SAVE_IF,
      `          cache-save-if: ${expression("github.event_name != 'merge_group'")}\n`,
      "saves on a pull request's ref: its `cache-save-if`",
    ],
    ["uv's save input left unset", UV_SAVE, "", "leaves `save-cache` unset"],
    [
      "uv's restore off on pull requests",
      UV_SAVE,
      `${UV_SAVE}          restore-cache: ${expression("github.event_name == 'push'")}\n`,
      "restores nothing on a pull request: its `restore-cache`",
    ],
    [
      "uv's cache never saved",
      UV_SAVE,
      "          save-cache: false\n",
      "does not save on a push to `main`",
    ],
    [
      "the release program saved on any miss",
      RELEASE_SAVE_IF,
      "      - if: steps.release-program-cache.outputs.cache-hit != 'true'\n",
      "saves on a pull request's ref (cache-hit false)",
    ],
    [
      "the release program saved on pull requests",
      RELEASE_SAVE_IF,
      "      - if: github.ref == 'refs/heads/main' || github.event_name == 'pull_request'\n",
      "saves on a pull request's ref (cache-hit true)",
    ],
    [
      "Bun's cache on",
      "          no-cache: true\n",
      "          no-cache: false\n",
      "restores and saves as one",
    ],
    [
      "the combined cache action",
      "        uses: actions/cache/restore@v4\n",
      "        uses: actions/cache@v4\n",
      "restores and saves as one",
    ],
    [
      "Node's cache asked for",
      "      - uses: actions/setup-node@v4\n",
      "      - uses: actions/setup-node@v4\n        with:\n          cache: npm\n",
      "restores and saves as one",
    ],
    [
      "an action nobody classified",
      "      - uses: extractions/setup-just@v3\n",
      "      - uses: example/setup-just-cached@v1\n",
      "does not know whether it caches",
    ],
  ];
  for (const [name, old, replacement, naming] of cases) {
    test(name, () => {
      const found = brokenFindings(old, replacement);
      assert.ok(found.includes(naming), `expected a finding naming ${naming}; got:\n${found}`);
    });
  }
});

describe("a lookup that cannot match main's cache is refused", () => {
  const cases = [
    [
      "a Rust cache key read off the ref",
      SHARD_KEY,
      `          cache-key: shard-${expression("matrix.platform.shard")}-${expression("github.ref")}\n`,
      "`cache-key` renders differently on a pull request",
    ],
    [
      "a uv cache suffix read off the event",
      UV_SAVE,
      `${UV_SAVE}          cache-suffix: ${expression("github.event_name")}\n`,
      "`cache-suffix` renders differently on a pull request",
    ],
    [
      "a restore key read off the base",
      RELEASE_KEY,
      `          key: release-program-${expression("matrix.platform.id")}-${expression("github.base_ref")}\n`,
      "`key` renders differently on a pull request",
    ],
    [
      "restore keys read off the ref",
      RELEASE_KEY,
      `${RELEASE_KEY}          restore-keys: release-program-${expression("github.ref")}-\n`,
      "`restore-keys` renders differently on a pull request",
    ],
    [
      "a restore run on pushes alone",
      "      - id: release-program-cache\n        uses: actions/cache/restore@v4\n",
      "      - id: release-program-cache\n        if: github.event_name == 'push'\n        uses: actions/cache/restore@v4\n",
      "restores nothing on a pull request",
    ],
    [
      "a save under another key",
      `${RELEASE_SAVE_IF}        uses: actions/cache/save@v4\n        with:\n          path: .release-program\n${RELEASE_KEY}`,
      `${RELEASE_SAVE_IF}        uses: actions/cache/save@v4\n        with:\n          path: .release-program\n          key: release-program-saved-${expression("matrix.platform.id")}\n`,
      "saves under a key no restore step before it looks up",
    ],
  ];
  for (const [name, old, replacement, naming] of cases) {
    test(name, () => {
      const found = brokenFindings(old, replacement);
      assert.ok(found.includes(naming), `expected a finding naming ${naming}; got:\n${found}`);
    });
  }
});
