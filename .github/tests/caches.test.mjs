// A pull request restores the caches `warm.yml` saves on every push to `main`,
// and saves none of its own.
//
// GitHub scopes a cache saved by a pull request's run to that pull request's
// merge ref, so no other run restores it. The repository's quota is one 10 GB
// pool, so open pull requests saving their caches evict `main`'s, which are the
// only caches every pull request restores, and every pull request after them
// runs cold. So each cache step of the two committed workflows is read here as
// GitHub runs it, for each matrix cell of its job: `ci.yml`'s under a pull
// request, `warm.yml`'s under a push to `main`. Its conditions and inputs are
// evaluated by `graph.mjs`, over GitHub's own published expression library.
//
// * Under a pull request every cache step of `ci.yml` restores where its
//   condition runs, and saves nothing, hit or miss.
// * Under a push to `main` every cache `warm.yml` keeps is saved where its key
//   missed.
// * Each `warm.yml` leg saves exactly the caches the `ci.yml` `test` cell of
//   its platform restores, under the keys that cell looks up, and every `test`
//   cell has a leg. Every other `ci.yml` job on a runner a leg runs on looks up
//   only what that leg saves. The macOS job has no leg, and compiles cold.
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
// of the account's monthly LFS bandwidth. On every push to `main` a `warm.yml`
// leg restores it, which keeps the entry from the forge's 7-day eviction, and
// fetches and saves it when the key missed. Every restore of it falls back by
// key prefix to an older object set's store.

import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, test } from "node:test";
import { data } from "@actions/expressions";
import { condition, readWorkflow, template } from "../lib/graph.mjs";
import { isObject } from "../lib/util.mjs";
import { WORKFLOWS } from "./support/lifecycle.mjs";

/** The run each workflow's cache steps are read under, by the `github` context each hands them. */
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
  "actions/download-artifact",
  "actions/upload-artifact",
  "extractions/setup-just",
  "nickderobertis/notignored",
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
/** The cache action's name, for an entry either of its halves keeps. */
const CACHE = "actions/cache";
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
   * @param {string} file
   * @param {string} job
   * @param {Record<string, any>} values
   * @param {Record<string, any>[]} steps
   * @param {Record<string, any>} env the job's own `env`
   * @param {unknown} when the job's own `if`
   * @param {unknown} runsOn the job's own `runs-on`
   */
  constructor(file, job, values, steps, env, when, runsOn) {
    this.file = file;
    this.job = job;
    this.values = values;
    this.steps = steps;
    this.env = env;
    this.when = when;
    this.runner = String(template(String(runsOn), { matrix: { platform: values } }, file));
  }

  toString() {
    return `${this.file} job \`${this.job}\`${"id" in this.values ? ` (${this.values.id})` : ""}`;
  }

  /**
   * The contexts a step of this cell is evaluated in, under one run. Every
   * step output the job reads stands for itself, so it renders the same on
   * every run; `outputs` overrides a step's outputs, such as a restore step's
   * `cache-hit`. The runner's own context renders per runner label, as it does
   * per runner image. The job's `env` is rendered under the run, each variable
   * its steps read and no other.
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
      runner: { os: `<os of ${this.runner}>`, arch: `<arch of ${this.runner}>` },
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
   * Whether the job runs under one run.
   *
   * @param {{github: Record<string, string>}} run
   */
  jobRuns(run) {
    return condition(this.when, { github: run.github, needs: {} }, [], `${this}`);
  }
}

/**
 * Each cell a workflow's jobs run as: one per `platform` matrix entry, or the job alone.
 *
 * @param {{file: string, jobs: Record<string, any>}} workflow
 */
function cells(workflow) {
  return Object.entries(workflow.jobs).flatMap(([name, job]) => {
    const steps = (job.steps ?? []).map((/** @type {Record<string, any>} */ step) => ({
      ...step,
      with: isObject(step.with) ? step.with : {},
    }));
    const env = isObject(job.env) ? job.env : {};
    const entries = job.strategy?.matrix?.platform;
    const cell = (/** @type {Record<string, any>} */ values) =>
      new Cell(workflow.file, name, values, steps, env, job.if, job["runs-on"]);
    return Array.isArray(entries) ? entries.map(cell) : [cell({})];
  });
}

/** @param {Record<string, any>} step */
function action(step) {
  return String(step.uses ?? "").split("@")[0];
}

/** @param {Record<string, any>} step */
function isStore(step) {
  return String(step.with.path) === EXEMPT;
}

/**
 * Whether a caching action's step keeps a cache at all, and what is wrong with
 * how it says so before either run is read.
 *
 * @param {Cell} cell
 * @param {Record<string, any>} step
 * @param {string} where
 * @param {Caching} caching
 * @param {{github: Record<string, string>}} run
 * @returns {{keeps: boolean, found: string[]}}
 */
function keeping(cell, step, where, caching, run) {
  const { onlyWith, off, save } = caching;
  if (onlyWith !== null && !(onlyWith in step.with)) {
    return { keeps: false, found: [] };
  }
  if (off && off.input in step.with && cell.input(step, off.input, run) === off.value) {
    return { keeps: false, found: [] };
  }
  if (save === null) {
    return {
      keeps: true,
      found: [
        `${where} caches, and restores and saves as one, so it cannot restore on a pull request without saving there${off ? `: set \`${off.input}: ${off.value}\`` : ""}`,
      ],
    };
  }
  if (!(save in step.with)) {
    return {
      keeps: true,
      found: [
        `${where} leaves \`${save}\` unset, so whether it saves on a pull request's ref is the action's default`,
      ],
    };
  }
  return { keeps: true, found: [] };
}

/**
 * Every way one cell's cache steps do not restore and save as its workflow's
 * run should: `ci.yml`'s restore and never save under a pull request, but the
 * suite's store; `warm.yml`'s save, under a push to `main`, what missed.
 *
 * @param {Cell} cell
 * @param {"pullRequest" | "main"} under
 * @returns {string[]}
 */
function cellFindings(cell, under) {
  const run = RUNS[under];
  /** @type {string[]} */
  const found = [];
  /** @type {Map<string, string>} the key each restore step looks up, by its id */
  const restored = new Map();
  cell.steps.forEach((step, index) => {
    const used = action(step);
    const where = `${cell} step ${index + 1} (\`${step.uses}\`)`;
    const caching = CACHING[used];
    if (!used || CACHELESS.has(used)) {
      return;
    }
    if (used === RESTORE) {
      // The store's restore is held by the store's own findings: a leg other
      // than the one keeping it is meant to skip it.
      if (!isStore(step) && !cell.runs(step, run)) {
        found.push(`${where} restores nothing on a ${run.label}: its condition skips it`);
      }
      restored.set(String(step.id), cell.input(step, "key", run));
    } else if (used === SAVE) {
      const key = cell.input(step, "key", run);
      const pair = [...restored].find(([, looked]) => looked === key)?.[0];
      if (pair === undefined) {
        found.push(`${where} saves under a key no restore step before it looks up`);
      } else if (isStore(step)) {
        // The store's save is held by the store's own findings.
      } else if (under === "pullRequest") {
        for (const answer of ["true", "false"]) {
          if (cell.runs(step, run, hit(pair, answer))) {
            found.push(`${where} saves on a pull request's ref (cache-hit ${answer})`);
          }
        }
      } else {
        if (!cell.runs(step, run, hit(pair, "false"))) {
          found.push(`${where} does not save on a push to \`main\` whose lookup missed`);
        }
        if (cell.runs(step, run, hit(pair, "true"))) {
          found.push(`${where} saves again on a push to \`main\` whose lookup hit`);
        }
      }
    } else if (caching === undefined) {
      found.push(`${where} uses an action this test does not know whether it caches`);
    } else {
      const kept = keeping(cell, step, where, caching, run);
      found.push(...kept.found);
      if (!kept.keeps || kept.found.length || caching.save === null) {
        return;
      }
      if (
        caching.restore !== null &&
        caching.restore in step.with &&
        cell.input(step, caching.restore, run) === "false"
      ) {
        found.push(
          `${where} restores nothing on a ${run.label}: its \`${caching.restore}\` renders \`false\``,
        );
      }
      const saves = cell.input(step, caching.save, run);
      if (under === "pullRequest" && saves !== "false") {
        found.push(
          `${where} saves on a pull request's ref: its \`${caching.save}\` does not render \`false\``,
        );
      }
      if (under === "main" && saves !== "true") {
        found.push(
          `${where} does not save on a push to \`main\`: its \`${caching.save}\` does not render \`true\``,
        );
      }
    }
  });
  return found;
}

/**
 * One cache a step looks up or saves, as everything its entry is keyed by: the
 * runner it runs on, the action, and each input its key is made of.
 *
 * @param {Cell} cell
 * @param {Record<string, any>} step
 * @param {string[]} inputs
 * @param {{github: Record<string, string>}} run
 * @param {Outputs} [outputs]
 */
function entry(cell, step, inputs, run, outputs) {
  const keyed = inputs
    .filter((input) => input in step.with)
    .map((input) => `${input}=${cell.input(step, input, run, outputs)}`);
  return `${action(step)} on ${cell.runner}${keyed.length ? `: ${keyed.join(", ")}` : ""}`;
}

/**
 * The caches a cell looks up under one run, the real-print store aside.
 *
 * @param {Cell} cell
 * @param {{github: Record<string, string>}} run
 * @returns {string[]}
 */
function lookups(cell, run) {
  return cell.steps.flatMap((step) => {
    const used = action(step);
    if (used === RESTORE) {
      return !isStore(step) && cell.runs(step, run)
        ? [entry(cell, step, ["path", "key"], run).replace(RESTORE, CACHE)]
        : [];
    }
    const caching = CACHING[used];
    if (caching === undefined || caching.save === null || !cell.runs(step, run)) {
      return [];
    }
    return keeping(cell, step, "", caching, run).keeps
      ? [entry(cell, step, caching.keyedBy, run)]
      : [];
  });
}

/**
 * The caches a cell saves under a push to `main` whose lookups all missed, the real-print store aside.
 *
 * @param {Cell} cell
 * @returns {string[]}
 */
function saved(cell) {
  const run = RUNS.main;
  const missed = Object.fromEntries(
    cell.steps
      .filter((step) => action(step) === RESTORE)
      .map((step) => [String(step.id), { "cache-hit": "false" }]),
  );
  return cell.steps.flatMap((step) => {
    const used = action(step);
    if (used === SAVE) {
      return !isStore(step) && cell.runs(step, run, missed)
        ? [entry(cell, step, ["path", "key"], run, missed).replace(SAVE, CACHE)]
        : [];
    }
    const caching = CACHING[used];
    if (caching === undefined || caching.save === null || !cell.runs(step, run)) {
      return [];
    }
    const kept = keeping(cell, step, "", caching, run);
    return kept.keeps && cell.input(step, caching.save, run) === "true"
      ? [entry(cell, step, caching.keyedBy, run)]
      : [];
  });
}

/**
 * Every way `warm.yml`'s legs do not save what `ci.yml`'s jobs look up.
 *
 * @param {Cell[]} ci
 * @param {Cell[]} warm
 * @returns {string[]}
 */
function keyFindings(ci, warm) {
  const found = [];
  const tests = ci.filter((cell) => cell.job === "test");
  const ids = (list) => JSON.stringify(list.map((cell) => [cell.values.id, cell.runner]));
  if (ids(tests) !== ids(warm)) {
    found.push(`warm.yml's legs ${ids(warm)} are not ci.yml's \`test\` cells ${ids(tests)}`);
  }
  for (const cell of tests) {
    const leg = warm.find((one) => one.values.id === cell.values.id);
    if (leg === undefined) {
      found.push(`${cell} has no warm leg`);
      continue;
    }
    const restores = lookups(cell, RUNS.pullRequest);
    const saves = saved(leg);
    for (const one of restores.filter((looked) => !saves.includes(looked))) {
      found.push(`${cell} restores \`${one}\`, which ${leg} does not save`);
    }
    for (const one of saves.filter((kept) => !restores.includes(kept))) {
      found.push(`${leg} saves \`${one}\`, which ${cell} does not restore`);
    }
  }
  for (const cell of ci.filter((one) => one.job !== "test")) {
    const legs = warm.filter((leg) => leg.runner === cell.runner);
    // A runner no leg runs on is the macOS job's: it compiles cold.
    if (legs.length === 0) {
      continue;
    }
    const saves = legs.flatMap(saved);
    for (const one of lookups(cell, RUNS.pullRequest).filter((looked) => !saves.includes(looked))) {
      found.push(`${cell} restores \`${one}\`, which no warm leg saves`);
    }
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
    const restore = cell.steps.find((step) => action(step) === RESTORE && isStore(step));
    const outputs = restore ? hit(String(restore.id), answer) : {};
    this.restores = restore !== undefined && cell.runs(restore, run, outputs);
    this.fetches = cell.steps.some((step) => step.run === FETCH && cell.runs(step, run, outputs));
    const save = cell.steps.find((step) => action(step) === SAVE && isStore(step));
    this.saves = save !== undefined && cell.runs(save, run, outputs);
    this.keys = [restore, save].map((step) => (step ? cell.input(step, "key", run, outputs) : ""));
  }

  /** What it did, as `[restores, fetches, saves]`. */
  get did() {
    return [this.restores, this.fetches, this.saves];
  }
}

/**
 * Every way the real-print store is not restored, warmed and saved as the header says.
 *
 * @param {Cell[]} ci
 * @param {Cell[]} warm
 * @returns {string[]}
 */
function storeFindings(ci, warm) {
  const found = [];
  const live = warm.filter((cell) => cell.jobRuns(RUNS.main));
  const warmed = live
    .map((cell) => new StoreRun(cell, RUNS.main, "false"))
    .filter((one) => one.restores && one.fetches && one.saves);
  if (warmed.length === 0) {
    found.push(
      "no job restores, fetches and saves the real-print store on a push to `main` whose key missed",
    );
  }
  if (!live.some((cell) => new StoreRun(cell, RUNS.main, "true").restores)) {
    found.push(
      "no job restores the real-print store on a push to `main` whose key hit, so its entry ages out",
    );
  }
  for (const cell of live.filter((one) => new StoreRun(one, RUNS.main, "true").saves)) {
    found.push(`${cell} saves the real-print store on a push to \`main\` whose key hit`);
  }
  const suite = ci.filter((cell) => cell.job === "skilltest");
  if (suite.length === 0) {
    found.push("no `skilltest` job reads the real-print store");
  }
  for (const cell of suite.filter((one) => one.jobRuns(RUNS.pullRequest))) {
    const missed = new StoreRun(cell, RUNS.pullRequest, "false");
    if (!missed.restores) {
      found.push(`${cell} does not restore the real-print store on a pull request`);
    } else if (warmed.length && !warmed.some((one) => one.keys[1] === missed.keys[0])) {
      found.push(`${cell}'s lookup of the real-print store is not the key main's warming saves`);
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
  for (const cell of ci.filter((one) => one.job !== "skilltest")) {
    for (const answer of ["true", "false"]) {
      if (new StoreRun(cell, RUNS.pullRequest, answer).saves) {
        found.push(
          `${cell} saves the real-print store on a pull request's ref (cache-hit ${answer})`,
        );
      }
    }
  }
  return found.concat([...ci, ...warm].flatMap((cell) => fallbackFindings(cell)));
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
    .filter((step) => action(step) === RESTORE && isStore(step))
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

/**
 * Every way the two workflows' caches are not restored by pull requests and saved from `main`.
 *
 * @param {{file: string, jobs: Record<string, any>}} ciWorkflow
 * @param {{file: string, jobs: Record<string, any>}} warmWorkflow
 * @returns {string[]}
 */
function findings(ciWorkflow, warmWorkflow) {
  const ci = cells(ciWorkflow);
  const warm = cells(warmWorkflow);
  return [
    ...ci.flatMap((cell) => cellFindings(cell, "pullRequest")),
    ...warm.flatMap((cell) => cellFindings(cell, "main")),
    ...keyFindings(ci, warm),
    ...storeFindings(ci, warm),
  ];
}

/** The two workflows as committed. */
const COMMITTED = {
  "ci.yml": readFileSync(join(WORKFLOWS, "ci.yml"), "utf8"),
  "warm.yml": readFileSync(join(WORKFLOWS, "warm.yml"), "utf8"),
};

/**
 * The findings over the committed workflows with one passage of one replaced, or every copy of it.
 *
 * @param {"ci.yml" | "warm.yml"} file
 * @param {string} old
 * @param {string} replacement
 * @param {boolean} [everywhere]
 */
function brokenFindings(file, old, replacement, everywhere = false) {
  const text = COMMITTED[file];
  assert.ok(text.includes(old), `the committed ${file} carries ${JSON.stringify(old)}`);
  const directory = mkdtempSync(join(tmpdir(), "caches-"));
  for (const [name, committed] of Object.entries(COMMITTED)) {
    writeFileSync(join(directory, name), committed);
  }
  const broken = everywhere ? text.replaceAll(old, replacement) : text.replace(old, replacement);
  writeFileSync(join(directory, file), broken);
  return findings(readWorkflow(directory, "ci.yml"), readWorkflow(directory, "warm.yml")).join(
    "\n",
  );
}

/** An Actions expression as a workflow writes it, without spelling its opener in a literal. */
function expression(inner) {
  return `${"$"}{{ ${inner} }}`;
}

const CI = () => readWorkflow(WORKFLOWS, "ci.yml");
const WARM = () => readWorkflow(WORKFLOWS, "warm.yml");

const RUST_SAVE_OFF = "          cache-save-if: false\n";
const RUST_SAVE_ON = "          cache-save-if: true\n";
const UV_SAVE_OFF = "          save-cache: false\n";
const UV_SAVE_ON = "          save-cache: true\n";
const SHARED_KEY = `          cache-shared-key: ${expression("matrix.platform.id")}\n`;
const RELEASE_KEY = `          key: release-program-${expression("matrix.platform.id")}-${expression("steps.release-program.outputs.key")}\n`;
const RELEASE_RESTORE =
  "      - uses: actions/cache/restore@v4\n        with:\n          path: .release-program\n";
const RELEASE_SAVE_IF = "      - if: steps.release-program-cache.outputs.cache-hit != 'true'\n";
const BUN_SETUP = "      - uses: oven-sh/setup-bun@v2\n        with:\n          no-cache: true\n";
const BUN_SAVE_IF = "      - if: steps.bun-cache.outputs.cache-hit != 'true'\n";
const BUN_SAVE = `${BUN_SAVE_IF}        uses: actions/cache/save@v4\n        with:\n          path: ~/.bun/bin\n          key: bun-${expression("runner.os")}-${expression("runner.arch")}-${expression("hashFiles('package.json')")}\n`;
const STORE_KEY = `          key: ${expression("steps.real-prints-key.outputs.key")}\n`;
const STORE_SAVE_IF =
  "      - if: env.REAL_PRINTS_STORE == 'true' && steps.real-prints-cache.outputs.cache-hit != 'true'\n        uses: actions/cache/save@v4\n";
const STORE_RESTORE_KEYS = "          restore-keys: real-prints-lfs-\n";
const WARMER_SWITCH = `      REAL_PRINTS_STORE: ${expression("matrix.platform.id == 'linux-x86_64'")}\n`;
const SUITE_SWITCH = '      REAL_PRINTS_STORE: "true"\n';

describe("the committed caches", () => {
  test("ci.yml restores on a pull request and saves nothing but the store, and warm.yml saves what it restores", () => {
    assert.deepEqual(findings(CI(), WARM()), []);
  });

  test("ci.yml keeps every cache it claims to, so the walk over it reads something", () => {
    const steps = cells(CI()).flatMap((cell) => cell.steps);
    const used = new Set(steps.map(action));
    for (const kept of ["actions-rust-lang/setup-rust-toolchain", "astral-sh/setup-uv", RESTORE]) {
      assert.ok(used.has(kept), `a step uses ${kept}`);
    }
    for (const step of steps.filter((one) => action(one) === "oven-sh/setup-bun")) {
      assert.equal(step.with["no-cache"], true, "a Bun step's `no-cache`");
    }
    assert.deepEqual(
      steps.filter((step) => action(step) === SAVE).map((step) => step.with.path),
      [EXEMPT],
      "the real-print store's save is ci.yml's one save step",
    );
  });

  test("warm.yml saves every cache a test cell restores, so the comparison reads something", () => {
    for (const leg of cells(WARM())) {
      const kept = saved(leg).map((one) =>
        one
          .replace(/ on [^:]+/, "")
          .replace(/, key=.*$/, "")
          .replace(/: cache-shared-key=.*$/, ""),
      );
      assert.deepEqual(
        kept.sort(),
        [
          "actions-rust-lang/setup-rust-toolchain",
          "astral-sh/setup-uv",
          `${CACHE}: path=~/.bun/bin`,
          `${CACHE}: path=.release-program`,
        ].sort(),
        `${leg}`,
      );
    }
  });

  test("every test cell has a warm leg on its own runner, and the macOS job none", () => {
    const tests = cells(CI()).filter((cell) => cell.job === "test");
    const legs = cells(WARM());
    assert.deepEqual(
      legs.map((leg) => [leg.values.id, leg.runner]),
      tests.map((cell) => [cell.values.id, cell.runner]),
    );
    const mac = cells(CI()).find((cell) => cell.job === "test-macos");
    assert.ok(mac, "the macOS job");
    assert.ok(!legs.some((leg) => leg.runner === mac.runner), "no leg runs on the macOS runner");
  });
});

describe("the committed real-print store", () => {
  const legs = cells(WARM());
  const keeper = legs.find((cell) => cell.values.id === "linux-x86_64");
  const suite = cells(CI()).find((cell) => cell.job === "skilltest");
  assert.ok(keeper && suite, "the linux-x86_64 leg and the suite's job");

  test("the linux-x86_64 leg restores it on every push to main, and on a miss fetches and saves it", () => {
    assert.equal(keeper.jobRuns(RUNS.main), true, "the leg runs on every push to main");
    assert.deepEqual(new StoreRun(keeper, RUNS.main, "false").did, [true, true, true]);
    assert.deepEqual(new StoreRun(keeper, RUNS.main, "true").did, [true, false, false]);
  });

  test("no other leg touches it", () => {
    for (const leg of legs.filter((one) => one !== keeper)) {
      for (const answer of ["true", "false"]) {
        assert.deepEqual(new StoreRun(leg, RUNS.main, answer).did, [false, false, false], `${leg}`);
      }
    }
  });

  test("a pull request's suite restores it, and saves it under its key only on a miss", () => {
    const missed = new StoreRun(suite, RUNS.pullRequest, "false");
    assert.deepEqual(missed.did, [true, true, true]);
    assert.equal(missed.keys[1], missed.keys[0], "the key it saves under is the one it missed");
    assert.deepEqual(new StoreRun(suite, RUNS.pullRequest, "true").did, [true, false, false]);
  });

  test("a pull request's lookup is the key main's warming saves", () => {
    const warmed = new StoreRun(keeper, RUNS.main, "false").keys[1];
    assert.equal(new StoreRun(suite, RUNS.pullRequest, "false").keys[0], warmed);
  });

  test("every restore of it falls back by prefix to an older object set's store", () => {
    assert.deepEqual(fallbackFindings(keeper), []);
    assert.deepEqual(fallbackFindings(suite), []);
  });
});

/** @type {["ci.yml" | "warm.yml", string, string, string, string, boolean?][]} */
const REFUSED = [
  // A pull request's ref saving a cache.
  [
    "ci.yml",
    "the Rust cache saved on a pull request",
    RUST_SAVE_OFF,
    RUST_SAVE_ON,
    "saves on a pull request's ref: its `cache-save-if`",
  ],
  [
    "ci.yml",
    "the Rust cache's save input left unset",
    RUST_SAVE_OFF,
    "",
    "leaves `cache-save-if` unset",
  ],
  ["ci.yml", "uv's save input left unset", UV_SAVE_OFF, "", "leaves `save-cache` unset"],
  [
    "ci.yml",
    "uv saved on a pull request",
    UV_SAVE_OFF,
    `          save-cache: ${expression("github.event_name == 'pull_request'")}\n`,
    "saves on a pull request's ref: its `save-cache`",
  ],
  [
    "ci.yml",
    "uv's restore off on pull requests",
    UV_SAVE_OFF,
    `${UV_SAVE_OFF}          restore-cache: ${expression("github.event_name == 'push'")}\n`,
    "restores nothing on a pull request: its `restore-cache`",
  ],
  [
    "ci.yml",
    "Bun's executable saved on a pull request's miss",
    `${BUN_SETUP}      - run: just runner-temp\n`,
    `${BUN_SETUP}${BUN_SAVE}      - run: just runner-temp\n`,
    "saves on a pull request's ref (cache-hit false)",
  ],
  [
    "ci.yml",
    "the release program saved on a pull request",
    "      - run: just octoprint-up\n",
    `      - uses: actions/cache/save@v4\n        with:\n          path: .release-program\n${RELEASE_KEY}      - run: just octoprint-up\n`,
    "saves on a pull request's ref (cache-hit true)",
  ],
  [
    "ci.yml",
    "Bun's cache on",
    "          no-cache: true\n",
    "          no-cache: false\n",
    "restores and saves as one",
  ],
  [
    "ci.yml",
    "the combined cache action",
    "        uses: actions/cache/restore@v4\n",
    "        uses: actions/cache@v4\n",
    "restores and saves as one",
  ],
  [
    "ci.yml",
    "Node's cache asked for",
    "        uses: actions/setup-node@v4\n",
    "        uses: actions/setup-node@v4\n        with:\n          cache: npm\n",
    "restores and saves as one",
  ],
  [
    "ci.yml",
    "an action nobody classified",
    "      - uses: extractions/setup-just@v3\n",
    "      - uses: example/setup-just-cached@v1\n",
    "does not know whether it caches",
  ],
  // A pull request's lookup that cannot match what `main` saves.
  [
    "ci.yml",
    "a Rust cache key read off the ref",
    `${SHARED_KEY}${RUST_SAVE_OFF}`,
    `${SHARED_KEY}          cache-key: ${expression("github.ref")}\n${RUST_SAVE_OFF}`,
    "which warm.yml job `warm` (linux-x86_64) does not save",
  ],
  [
    "ci.yml",
    "a uv cache suffix read off the event",
    UV_SAVE_OFF,
    `${UV_SAVE_OFF}          cache-suffix: ${expression("github.event_name")}\n`,
    "which no warm leg saves",
  ],
  [
    "ci.yml",
    "a restore key read off the base",
    RELEASE_KEY,
    `          key: release-program-${expression("matrix.platform.id")}-${expression("github.base_ref")}\n`,
    "which warm.yml job `warm` (linux-x86_64) does not save",
  ],
  [
    "ci.yml",
    "a restore run on pushes alone",
    RELEASE_RESTORE,
    "      - if: github.event_name == 'push'\n        uses: actions/cache/restore@v4\n        with:\n          path: .release-program\n",
    "restores nothing on a pull request: its condition skips it",
  ],
  [
    "ci.yml",
    "the lint job's Rust cache keyed apart from Linux's leg",
    "          cache-shared-key: linux-x86_64\n",
    "          cache-shared-key: lint\n",
    "ci.yml job `lint` restores `actions-rust-lang/setup-rust-toolchain on ubuntu-24.04: cache-shared-key=lint`, which no warm leg saves",
  ],
  // `main` not saving what a pull request restores.
  [
    "warm.yml",
    "warm's Rust cache keyed apart from the test cell",
    SHARED_KEY,
    `          cache-shared-key: warm-${expression("matrix.platform.id")}\n`,
    "which ci.yml job `test` (linux-x86_64) does not restore",
  ],
  [
    "warm.yml",
    "warm's Rust cache never saved",
    RUST_SAVE_ON,
    RUST_SAVE_OFF,
    "does not save on a push to `main`: its `cache-save-if` does not render `true`",
  ],
  [
    "warm.yml",
    "warm's uv cache never saved",
    UV_SAVE_ON,
    UV_SAVE_OFF,
    "does not save on a push to `main`: its `save-cache` does not render `true`",
  ],
  [
    "warm.yml",
    "Bun's executable saved on a hit alone",
    BUN_SAVE_IF,
    "      - if: steps.bun-cache.outputs.cache-hit == 'true'\n",
    "does not save on a push to `main` whose lookup missed",
  ],
  [
    "warm.yml",
    "the release program saved again on a hit",
    RELEASE_SAVE_IF,
    "      - if: always()\n",
    "saves again on a push to `main` whose lookup hit",
  ],
  [
    "warm.yml",
    "the release program saved under another key",
    `          path: .release-program\n${RELEASE_KEY}      # The steps`,
    `          path: .release-program\n          key: release-program-saved-${expression("matrix.platform.id")}\n      # The steps`,
    "saves under a key no restore step before it looks up",
  ],
  [
    "warm.yml",
    "a test cell with no leg",
    "          - id: windows-aarch64\n            runner: windows-11-arm\n",
    "",
    "ci.yml job `test` (windows-aarch64) has no warm leg",
  ],
  // The real-print store.
  [
    "warm.yml",
    "the store never warmed from main",
    WARMER_SWITCH,
    '      REAL_PRINTS_STORE: "false"\n',
    "no job restores, fetches and saves the real-print store on a push to `main` whose key missed",
  ],
  [
    "warm.yml",
    "the store saved on main's hit",
    STORE_SAVE_IF,
    "      - if: env.REAL_PRINTS_STORE == 'true'\n        uses: actions/cache/save@v4\n",
    "saves the real-print store on a push to `main` whose key hit",
  ],
  [
    "ci.yml",
    "the store saved on a pull request's hit",
    STORE_SAVE_IF,
    "      - if: env.REAL_PRINTS_STORE == 'true'\n        uses: actions/cache/save@v4\n",
    "saves the real-print store on a pull request whose key hit",
  ],
  [
    "ci.yml",
    "the store not restored by the suite's pull request",
    SUITE_SWITCH,
    '      REAL_PRINTS_STORE: "false"\n',
    "ci.yml job `skilltest` does not restore the real-print store on a pull request",
  ],
  [
    "ci.yml",
    "the suite looking the store up under a key main does not save",
    STORE_KEY,
    `          key: pr-${expression("steps.real-prints-key.outputs.key")}\n`,
    "lookup of the real-print store is not the key main's warming saves",
    true,
  ],
  [
    "warm.yml",
    "the store restored with no fallback",
    STORE_RESTORE_KEYS,
    "",
    "has no `restore-keys`, so a miss falls back to no older store",
  ],
  [
    "ci.yml",
    "the store falling back to nothing an older set's key begins with",
    STORE_RESTORE_KEYS,
    "          restore-keys: real-prints-store-\n",
    "prefix no older object set's store",
  ],
];

describe("a cache kept other than the header says is refused", () => {
  for (const [file, name, old, replacement, naming, everywhere] of REFUSED) {
    test(`${file}: ${name}`, () => {
      const found = brokenFindings(file, old, replacement, everywhere);
      assert.ok(found.includes(naming), `expected a finding naming ${naming}; got:\n${found}`);
    });
  }
});
