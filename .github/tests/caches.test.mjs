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
// and Bun's cache of its own executable, must not cache at all; Bun's executable
// is kept by the cache action's two halves around it instead. An action this
// module does not know is refused rather than assumed to keep nothing.
//
// One cache is exempt, and it is named: the real-print suite's LFS store, keyed
// on the cases' content. A pull request's `skilltest` restores it and saves it
// only when that key missed, so only a pull request changing the cases saves
// one, and its later pushes restore it rather than download the cases again out
// of the account's monthly LFS bandwidth. On every push to `main`, whatever the
// suite's classifier answers, a job restores it, which keeps the entry from the
// forge's 7-day eviction, and fetches and saves it when the key missed. Every
// restore of it falls back by key prefix to an older object set's store.

import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, test } from "node:test";
import { data } from "@actions/expressions";
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
/** The recipe that downloads the real-print cases into that store. */
const FETCH = "just real-prints-fetch";

/** @typedef {Record<string, Record<string, string>>} Outputs step outputs by step id */

/**
 * The functions a cache key may call beyond the library's own. `hashFiles`
 * stands for the digest of what it names, the same on both runs. It takes any
 * arguments rather than restating the runner's signature: which arguments the
 * runner accepts is actionlint's to hold, under `just lint-workflows`, and all
 * this reads is that one call renders alike under both runs.
 *
 * @type {Map<string, import("@actions/expressions/funcs/info").FunctionDefinition>}
 */
const FUNCTIONS = new Map([
  [
    "hashFiles",
    {
      name: "hashFiles",
      minArgs: 0,
      maxArgs: Number.MAX_SAFE_INTEGER,
      call: (...args) =>
        new data.StringData(`<hashFiles(${args.map((arg) => arg.coerceString()).join(", ")})>`),
    },
  ],
]);

/**
 * A restore step's `cache-hit`, as the outputs a context is given.
 *
 * @param {string} id
 * @param {string} hit
 * @returns {Outputs}
 */
function hit(id, hit) {
  return { [id]: { "cache-hit": hit } };
}

/** One matrix cell of one job, and the steps it runs. */
class Cell {
  /**
   * @param {string} job
   * @param {Record<string, any>} values
   * @param {Record<string, any>[]} steps
   * @param {Record<string, any>} env the job's own `env`
   * @param {unknown} when the job's own `if`
   */
  constructor(job, values, steps, env, when) {
    this.job = job;
    this.values = values;
    this.steps = steps;
    this.env = env;
    this.when = when;
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
   * both runs, as does the runner; `outputs` overrides a step's outputs, such
   * as a restore step's `cache-hit`. The job's `env` is rendered under the run,
   * each variable its steps read and no other.
   *
   * @param {{github: Record<string, string>}} run
   * @param {Outputs} [outputs]
   */
  contexts(run, outputs = {}) {
    /** @type {Record<string, {outputs: Record<string, string>}>} */
    const steps = {};
    for (const [, id, name] of JSON.stringify(this.steps).matchAll(
      /steps\.([\w-]+)\.outputs\.([\w-]+)/g,
    )) {
      steps[id] ??= { outputs: {} };
      steps[id].outputs[name] = `<${id}.${name}>`;
    }
    for (const [id, given] of Object.entries(outputs)) {
      steps[id] ??= { outputs: {} };
      Object.assign(steps[id].outputs, given);
    }
    const base = {
      github: run.github,
      matrix: { platform: this.values },
      runner: { os: "<runner.os>", arch: "<runner.arch>" },
    };
    /** @type {Record<string, string>} the job's variables its steps read, and no others */
    const env = {};
    for (const [, name] of JSON.stringify(this.steps).matchAll(/\benv\.([\w-]+)/g)) {
      const value = this.env[name];
      env[name] = value === undefined ? "" : String(template(String(value), base, `${this}`));
    }
    return { ...base, env, steps };
  }

  /**
   * An input as the step hands it to its action under one run, as the string the action reads.
   *
   * @param {Record<string, any>} step
   * @param {string} input
   * @param {{github: Record<string, string>}} run
   * @param {Outputs} [outputs]
   */
  input(step, input, run, outputs) {
    return String(
      template(String(step.with[input]), this.contexts(run, outputs), `${this}`, FUNCTIONS),
    );
  }

  /**
   * Whether the step runs under one run.
   *
   * @param {Record<string, any>} step
   * @param {{github: Record<string, string>}} run
   * @param {Outputs} [outputs]
   */
  runs(step, run, outputs) {
    return condition(step.if, this.contexts(run, outputs), [], `${this}`);
  }

  /**
   * Whether the job runs under one run, when every job it needs succeeded with `needs` outputs.
   *
   * @param {{github: Record<string, string>}} run
   * @param {Record<string, Record<string, string>>} needs
   */
  jobRuns(run, needs) {
    const context = Object.fromEntries(
      Object.entries(needs).map(([job, outputs]) => [job, { outputs, result: "success" }]),
    );
    const results = Object.keys(needs).map(() => "success");
    return condition(this.when, { github: run.github, needs: context }, results, `${this}`);
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
    const env = isObject(job.env) ? job.env : {};
    const entries = job.strategy?.matrix?.platform;
    return Array.isArray(entries)
      ? entries.map((entry) => new Cell(name, entry, steps, env, job.if))
      : [new Cell(name, {}, steps, env, job.if)];
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
  const all = cells(workflow);
  return all.flatMap((cell) => cellFindings(cell)).concat(storeFindings(all));
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
  // The store's restore is held by the store's own findings: a job warming it
  // on `main` is meant to skip it on a pull request.
  const exempt = String(step.with.path) === EXEMPT;
  for (const run of Object.values(RUNS)) {
    if (!exempt && !cell.runs(step, run)) {
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
  const exempt = String(step.with.path) === EXEMPT;
  if (!exempt && !cell.runs(step, RUNS.main, hit(pair, "false"))) {
    found.push(`${where} does not save on a push to \`main\` whose lookup missed`);
  }
  for (const answer of exempt ? ["true"] : ["true", "false"]) {
    if (cell.runs(step, RUNS.pullRequest, hit(pair, answer))) {
      found.push(`${where} saves on a pull request's ref (cache-hit ${answer})`);
    }
  }
  if (exempt && cell.runs(step, RUNS.main, hit(pair, "true"))) {
    found.push(`${where} saves the real-print store on a push to \`main\` whose key hit`);
  }
  return found;
}

/** What one cell does with the real-print store under one run and one lookup. */
class StoreRun {
  /**
   * @param {Cell} cell
   * @param {{github: Record<string, string>}} run
   * @param {string} answer the restore's `cache-hit`
   */
  constructor(cell, run, answer) {
    const restore = cell.steps.find(
      (step) => action(step) === RESTORE && String(step.with.path) === EXEMPT,
    );
    const outputs = restore ? hit(String(restore.id), answer) : {};
    this.restores = restore !== undefined && cell.runs(restore, run, outputs);
    this.fetches = cell.steps.some((step) => step.run === FETCH && cell.runs(step, run, outputs));
    const save = cell.steps.find(
      (step) => action(step) === SAVE && String(step.with.path) === EXEMPT,
    );
    this.saves = save !== undefined && cell.runs(save, run, outputs);
    this.keys = [restore, save].map((step) => (step ? cell.input(step, "key", run, outputs) : ""));
  }
}

/** The jobs a classifier decides, and the outputs each answer hands them. */
const CLASSIFIED = ["true", "false"].map((owed) => ({
  owed,
  needs: { "skilltest-classify": { owed, reason: "<reason>" } },
}));

/**
 * The cells that run under one run when the suite's classifier answered `owed`.
 *
 * @param {Cell[]} all
 * @param {{github: Record<string, string>}} run
 * @param {{needs: Record<string, Record<string, string>>}} classified
 */
function running(all, run, classified) {
  return all.filter(
    (cell) =>
      cell.steps.some((step) => String(step.with.path) === EXEMPT) &&
      cell.jobRuns(run, classified.needs),
  );
}

/**
 * Every way the real-print store is not restored, warmed and saved as the header says.
 *
 * @param {Cell[]} all
 * @returns {string[]}
 */
function storeFindings(all) {
  const found = [];
  for (const classified of CLASSIFIED) {
    const live = running(all, RUNS.main, classified);
    const missed = live.map((cell) => new StoreRun(cell, RUNS.main, "false"));
    if (!missed.some((one) => one.restores && one.fetches && one.saves)) {
      found.push(
        `no job restores, fetches and saves the real-print store on a push to \`main\` whose classifier answered owed=${classified.owed} and whose key missed`,
      );
    }
    const hits = live.map((cell) => new StoreRun(cell, RUNS.main, "true"));
    if (!hits.some((one) => one.restores)) {
      found.push(
        `no job restores the real-print store on a push to \`main\` whose classifier answered owed=${classified.owed}, so its entry ages out`,
      );
    }
  }
  const suite = all.filter((cell) => cell.job === "skilltest");
  const owed = CLASSIFIED.find((one) => one.owed === "true") ?? CLASSIFIED[0];
  for (const cell of suite.filter((one) => one.jobRuns(RUNS.pullRequest, owed.needs))) {
    const missed = new StoreRun(cell, RUNS.pullRequest, "false");
    if (!missed.restores) {
      found.push(`${cell} does not restore the real-print store on a pull request`);
    }
    if (!missed.saves) {
      found.push(`${cell} does not save the real-print store on a pull request whose key missed`);
    } else if (missed.keys[0] !== missed.keys[1]) {
      found.push(`${cell} saves the real-print store under a key other than the one it missed`);
    }
    if (new StoreRun(cell, RUNS.pullRequest, "true").saves) {
      found.push(`${cell} saves the real-print store on a pull request whose key hit`);
    }
  }
  if (suite.length === 0) {
    found.push("no `skilltest` job reads the real-print store");
  }
  return found.concat(all.flatMap((cell) => fallbackFindings(cell)));
}

/** Two cache keys the cases' key recipe could print: an older object set's, and a newer one's. */
const OBJECT_SETS = ["0".repeat(40), "f".repeat(40)].map((digest) => `real-prints-lfs-${digest}`);

/**
 * A restore of the real-print store falls back by prefix to an older object set's store.
 *
 * @param {Cell} cell
 */
function fallbackFindings(cell) {
  return cell.steps
    .filter((step) => action(step) === RESTORE && String(step.with.path) === EXEMPT)
    .flatMap((step) => {
      const where = `${cell}'s real-print store restore`;
      const keyed = /steps\.([\w-]+)\.outputs\.key/.exec(String(step.with.key));
      if (keyed === null) {
        return [`${where} is keyed on no step's \`key\` output`];
      }
      const [older, newer] = OBJECT_SETS;
      const outputs = { [keyed[1]]: { key: newer } };
      if (cell.input(step, "key", RUNS.main, outputs) !== newer) {
        return [`${where} looks up a key other than the one the key recipe prints`];
      }
      if (!("restore-keys" in step.with)) {
        return [`${where} has no \`restore-keys\`, so a miss falls back to no older store`];
      }
      const prefixes = cell.input(step, "restore-keys", RUNS.main, outputs).split("\n");
      return prefixes.some((prefix) => prefix.trim() && older.startsWith(prefix.trim()))
        ? []
        : [`${where}'s \`restore-keys\` prefix no older object set's store`];
    });
}

const TEXT = readFileSync(CI, "utf8");

/**
 * The findings over the committed workflow with one passage replaced, or every copy of it.
 *
 * @param {string} old
 * @param {string} replacement
 * @param {boolean} [everywhere]
 */
function brokenFindings(old, replacement, everywhere = false) {
  assert.ok(TEXT.includes(old), `the committed workflow carries ${JSON.stringify(old)}`);
  const directory = mkdtempSync(join(tmpdir(), "caches-"));
  const broken = everywhere ? TEXT.replaceAll(old, replacement) : TEXT.replace(old, replacement);
  writeFileSync(join(directory, "ci.yml"), broken);
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
const BUN_SAVE_IF =
  "      - if: github.ref == 'refs/heads/main' && steps.bun-cache.outputs.cache-hit != 'true'\n";
const STORE_SAVE_IF =
  "      - if: env.REAL_PRINTS_STORE == 'true' && steps.real-prints-cache.outputs.cache-hit != 'true'\n        uses: actions/cache/save@v4\n";
const STORE_RESTORE_KEYS = "          restore-keys: real-prints-lfs-\n";
const WARMER_SWITCH = `      REAL_PRINTS_STORE: ${expression("github.ref == 'refs/heads/main'")}\n`;
const SUITE_SWITCH = '      REAL_PRINTS_STORE: "true"\n';

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
    assert.ok(
      steps.some((step) => action(step) === SAVE && step.with.path === "~/.bun/bin"),
      "Bun's executable's save step",
    );
  });
});

describe("the committed ci.yml's real-print store", () => {
  const all = cells(readWorkflow(WORKFLOWS, "ci.yml"));
  const warmer = all.find((cell) => cell.job === "skilltest-classify");
  const suite = all.find((cell) => cell.job === "skilltest");
  assert.ok(warmer && suite, "the classifier's and the suite's jobs");

  for (const { owed, needs } of CLASSIFIED) {
    test(`a push to main restores it, and on a miss fetches and saves it, when owed=${owed}`, () => {
      const live = running(all, RUNS.main, { needs });
      assert.ok(live.includes(warmer), "the classifier's job runs on every push to main");
      const missed = new StoreRun(warmer, RUNS.main, "false");
      assert.deepEqual(
        [missed.restores, missed.fetches, missed.saves],
        [true, true, true],
        "restore, fetch and save on a miss",
      );
      const hitting = new StoreRun(warmer, RUNS.main, "true");
      assert.deepEqual(
        [hitting.restores, hitting.fetches, hitting.saves],
        [true, false, false],
        "restore alone on a hit",
      );
      assert.equal(suite.jobRuns(RUNS.main, needs), owed === "true", "the suite runs where owed");
    });
  }

  test("a pull request's classifier leaves it alone", () => {
    for (const answer of ["true", "false"]) {
      const run = new StoreRun(warmer, RUNS.pullRequest, answer);
      assert.deepEqual([run.restores, run.fetches, run.saves], [false, false, false]);
    }
  });

  test("a pull request's suite restores it, and saves it under its key only on a miss", () => {
    const missed = new StoreRun(suite, RUNS.pullRequest, "false");
    assert.deepEqual([missed.restores, missed.fetches, missed.saves], [true, true, true]);
    assert.equal(missed.keys[1], missed.keys[0], "the key it saves under is the one it missed");
    const hitting = new StoreRun(suite, RUNS.pullRequest, "true");
    assert.deepEqual([hitting.restores, hitting.fetches, hitting.saves], [true, false, false]);
  });

  test("a pull request's lookup is the key main's warming saves", () => {
    const warmed = new StoreRun(warmer, RUNS.main, "false").keys[1];
    assert.equal(new StoreRun(suite, RUNS.pullRequest, "false").keys[0], warmed);
  });

  test("every restore of it falls back by prefix to an older object set's store", () => {
    assert.deepEqual(fallbackFindings(warmer), []);
    assert.deepEqual(fallbackFindings(suite), []);
  });
});

describe("the real-print store kept other than the header says is refused", () => {
  const cases = [
    [
      "saved on a hit",
      STORE_SAVE_IF,
      "      - if: env.REAL_PRINTS_STORE == 'true'\n        uses: actions/cache/save@v4\n",
      "saves on a pull request's ref (cache-hit true)",
    ],
    [
      "never warmed from main",
      WARMER_SWITCH,
      '      REAL_PRINTS_STORE: "false"\n',
      "no job restores, fetches and saves the real-print store on a push to `main` whose classifier answered owed=false",
    ],
    [
      "not saved by the suite's pull request",
      SUITE_SWITCH,
      `      REAL_PRINTS_STORE: ${expression("github.ref == 'refs/heads/main'")}\n`,
      "job `skilltest` does not restore the real-print store on a pull request",
    ],
    [
      "restored with no fallback",
      STORE_RESTORE_KEYS,
      "",
      "has no `restore-keys`, so a miss falls back to no older store",
    ],
    [
      "falling back to nothing an older set's key begins with",
      STORE_RESTORE_KEYS,
      "          restore-keys: real-prints-store-\n",
      "prefix no older object set's store",
    ],
  ];
  for (const [name, old, replacement, naming] of cases) {
    test(name, () => {
      const found = brokenFindings(old, replacement, true);
      assert.ok(found.includes(naming), `expected a finding naming ${naming}; got:\n${found}`);
    });
  }
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
      "Bun's executable saved on any miss",
      BUN_SAVE_IF,
      "      - if: steps.bun-cache.outputs.cache-hit != 'true'\n",
      "saves on a pull request's ref (cache-hit false)",
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
