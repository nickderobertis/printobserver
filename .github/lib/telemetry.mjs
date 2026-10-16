// The telemetry `github-workflows:test` records and the two budget commands
// read: what it is computed from, and the two figures read off it.
//
// The record is only as current as the files it was computed from, so it
// carries a digest of exactly those files, and a reader recomputes it and
// refuses a record that no longer matches the tree.

import { createHash } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { isMacosLabel, isObject } from "./util.mjs";

/** Where the record is written, under `.github/`. */
export const TELEMETRY = join(".telemetry", "macos-policy.json");

/** What the record's shape is, for a reader that has to refuse another. */
export const SCHEMA = 1;

/**
 * Every file the record is computed from, repository-relative and sorted: the
 * workflows; the policy, its graph and its locked dependencies; the scenarios,
 * their event fixtures and the test that records them; and the tree files the
 * scenarios read — the release branch prefix, the platform list and the
 * recipe and module that resolve a dispatch's runner, and every file the
 * generated histories copy as their base commit.
 *
 * @param {string} root the repository root
 * @returns {string[]}
 */
export function digestFiles(root) {
  const under = (directory, pattern) =>
    readdirSync(join(root, directory), { recursive: true })
      .map(String)
      .filter((name) => pattern.test(name.split("\\").join("/")))
      .map((name) => join(directory, name));
  return [
    ...under(".github/workflows", /\.ya?ml$/),
    ...under(".github/lib", /\.mjs$/),
    ...under(".github/tests", /^(support\/[^/]+\.mjs|fixtures\/.+\.json|lifecycle\.test\.mjs)$/),
    ...under("crates", /^[^/]+\/Cargo\.toml$/),
    ".github/macos-policy.mjs",
    ".github/package.json",
    "package.json",
    "bun.lock",
    "release-plz.toml",
    "AGENTS.md",
    "justfile",
    "tools/repo-checks/src/repo_checks/dispatching.py",
    "Cargo.toml",
    "Cargo.lock",
    "crates/printobserver-core/CHANGELOG.md",
    "crates/printobserver-core/src/lib.rs",
  ]
    .map((path) => relative(root, join(root, path)).split("\\").join("/"))
    .sort();
}

/**
 * Why a record is not one the figures can be read off, or "" where it is.
 * Every scenario, run and job is the shape `github-workflows:test` writes,
 * and every run's count of macOS jobs is the count of its jobs on macOS.
 *
 * @param {unknown} record
 * @returns {string}
 */
export function invalid(record) {
  if (!isObject(record) || record.schema !== SCHEMA || !Array.isArray(record.scenarios)) {
    return `it is not schema ${SCHEMA} telemetry with a list of scenarios`;
  }
  // A record of no scenarios, or of none in the change's lifecycle, would read
  // as zero for both figures: an absent workload rather than a measured one.
  const lifecycle = record.scenarios.filter(
    (scenario) => isObject(scenario) && typeof scenario.lifecycle_step === "string",
  );
  if (record.scenarios.length === 0 || lifecycle.length === 0) {
    return "it records no scenario of a change's lifecycle, so it measured no workload";
  }
  for (const [at, scenario] of record.scenarios.entries()) {
    const where = `scenario ${at}`;
    if (
      !isObject(scenario) ||
      typeof scenario.id !== "string" ||
      typeof scenario.event_class !== "string" ||
      typeof scenario.manual !== "boolean" ||
      !(scenario.lifecycle_step === null || typeof scenario.lifecycle_step === "string") ||
      !Array.isArray(scenario.runs)
    ) {
      return `${where} lacks its id, event class, lifecycle step, manual flag or runs`;
    }
    for (const run of scenario.runs) {
      if (
        !isObject(run) ||
        typeof run.workflow !== "string" ||
        typeof run.event !== "string" ||
        typeof run.action !== "string" ||
        !Number.isInteger(run.macos_jobs) ||
        run.macos_jobs < 0 ||
        !Array.isArray(run.jobs)
      ) {
        return `${where} (${scenario.id}) has a run lacking its workflow, event, action, macOS count or jobs`;
      }
      const jobs = run.jobs.filter(
        (job) =>
          isObject(job) &&
          typeof job.job === "string" &&
          typeof job.display === "string" &&
          typeof job.macos === "boolean" &&
          typeof job.target === "string" &&
          Array.isArray(job.runner) &&
          job.runner.every((label) => typeof label === "string"),
      );
      if (jobs.length !== run.jobs.length) {
        return `${where} (${scenario.id}) has a job of ${run.workflow} that is not a job row`;
      }
      const mislabelled = jobs.find((job) => job.macos !== job.runner.some(isMacosLabel));
      if (mislabelled) {
        return `${where} (${scenario.id}) marks ${mislabelled.display} on ${mislabelled.runner.join("+")} as ${mislabelled.macos ? "" : "not "}macOS`;
      }
      if (jobs.filter((job) => job.macos).length !== run.macos_jobs) {
        return `${where} (${scenario.id}) counts ${run.macos_jobs} macOS job(s) in ${run.workflow} and lists another number`;
      }
    }
  }
  return "";
}

/**
 * @param {string} root
 * @returns {string}
 */
export function sourceDigest(root) {
  const hash = createHash("sha256");
  for (const path of digestFiles(root)) {
    hash.update(path);
    hash.update("\0");
    hash.update(readFileSync(join(root, path)));
    hash.update("\0");
  }
  return `sha256:${hash.digest("hex")}`;
}

/**
 * @typedef {{job: string, display: string, runner: string[], macos: boolean, target: string}} JobRow
 * @typedef {{workflow: string, event: string, action: string, macos_jobs: number, jobs: JobRow[]}} RunRow
 * @typedef {{id: string, event_class: string, diff: string, ownership: string, branch: string, lifecycle_step: string | null, manual: boolean, classification: object | null, runs: RunRow[]}} ScenarioRow
 */

/**
 * The largest number of macOS job instances any one run requests, over every
 * scenario — manual ones included, since a dispatch is a run too.
 *
 * @param {{scenarios: ScenarioRow[]}} record
 * @returns {{value: number, detail: string}}
 */
export function jobsPerRun(record) {
  const runs = record.scenarios.flatMap((scenario) =>
    scenario.runs.map((run) => ({ scenario, run })),
  );
  const value = Math.max(0, ...runs.map(({ run }) => run.macos_jobs));
  const spending = runs.filter(({ run }) => run.macos_jobs > 0);
  // One line per distinct shape of run rather than per scenario: forty pull
  // request scenarios requesting the same one job are one fact.
  const shapes = new Map();
  for (const { scenario, run } of spending) {
    const shape = `${run.workflow} on ${run.event}${run.action ? `.${run.action}` : ""}: ${run.jobs
      .filter((job) => job.macos)
      .map((job) => `${job.display} [${job.target || "dispatched"} on ${job.runner.join("+")}]`)
      .join(", ")}`;
    shapes.set(shape, [...(shapes.get(shape) ?? []), scenario.event_class]);
  }
  const lines = [...shapes].map(
    ([shape, classes]) =>
      `${shape} (${classes.length} scenario(s): ${[...new Set(classes)].join(", ")})`,
  );
  const perWorkflow = new Map();
  for (const { run } of runs) {
    perWorkflow.set(run.workflow, Math.max(perWorkflow.get(run.workflow) ?? 0, run.macos_jobs));
  }
  const summary = [...perWorkflow].map(([workflow, most]) => `${workflow}=${most}`).join(", ");
  return {
    value,
    detail: `most macOS jobs in one run, per workflow: ${summary}. Runs requesting macOS (${spending.length} of ${runs.length}): ${lines.join("; ") || "none"}`,
  };
}

/**
 * How many distinct runs over the automatic lifecycle of one change that
 * receives one push request a macOS runner: the pull request that push opens,
 * its merge, release automation's pull request and its merge, the release it
 * cuts, tags and publishes, the downstream proofs and the weekly schedules —
 * the scenarios marked with a lifecycle step. A further push to the same pull
 * request (`synchronize`) or a reopen is that pull request's run again, a
 * second push rather than this change's, and is recorded as a scenario of its
 * own with no lifecycle step. Manual diagnostics are outside the lifecycle.
 *
 * @param {{scenarios: ScenarioRow[]}} record
 * @returns {{value: number, detail: string}}
 */
export function runsPerChange(record) {
  const lifecycle = record.scenarios.filter(
    (scenario) => scenario.lifecycle_step && !scenario.manual,
  );
  const spending = lifecycle.flatMap((scenario) =>
    scenario.runs
      .filter((run) => run.macos_jobs > 0)
      .map(
        (run) =>
          `${scenario.lifecycle_step}: ${run.workflow} on ${run.event}${run.action ? `.${run.action}` : ""} (${run.jobs
            .filter((job) => job.macos)
            .map((job) => `${job.display} on ${job.target || "no target"}`)
            .join(", ")})`,
      ),
  );
  const steps = [...new Set(lifecycle.map((scenario) => scenario.lifecycle_step))].sort();
  return {
    value: spending.length,
    detail: `one-push lifecycle steps: ${steps.join(" -> ")}. Runs requesting macOS: ${spending.join("; ") || "none"}`,
  };
}
