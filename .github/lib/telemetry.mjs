// The telemetry `github-workflows:test` records and the two budget commands
// read: what it is computed from, and the two figures read off it.
//
// The record is only as current as the files it was computed from, so it
// carries a digest of exactly those files, and a reader recomputes it and
// refuses a record that no longer matches the tree.

import { createHash } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";

/** Where the record is written, under `.github/`. */
export const TELEMETRY = join(".telemetry", "macos-policy.json");

/** What the record's shape is, for a reader that has to refuse another. */
export const SCHEMA = 1;

/**
 * Every file the record is computed from, repository-relative and sorted: the
 * workflows, the policy and its graph, the scenarios and their event fixtures,
 * and the tree files the scenarios read (the release branch prefix, the
 * platform list the dispatch resolves against, the manifests the generated
 * histories copy).
 *
 * @param {string} root the repository root
 * @returns {string[]}
 */
export function digestFiles(root) {
  const under = (directory, pattern) =>
    readdirSync(join(root, directory), { recursive: true })
      .map(String)
      .filter((name) => pattern.test(name))
      .map((name) => join(directory, name));
  return [
    ...under(".github/workflows", /\.ya?ml$/),
    ...under(".github/lib", /\.mjs$/),
    ...under(".github/tests/support", /\.mjs$/),
    ...under(".github/tests/fixtures", /\.json$/),
    ".github/macos-policy.mjs",
    "release-plz.toml",
    "AGENTS.md",
    "Cargo.toml",
    "Cargo.lock",
  ]
    .map((path) => relative(root, join(root, path)).split("\\").join("/"))
    .sort();
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
 * How many distinct runs over one change's automatic lifecycle request a
 * macOS runner; manual diagnostics are outside the lifecycle.
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
    detail: `lifecycle steps: ${steps.join(" -> ")}. Runs requesting macOS: ${spending.join("; ") || "none"}`,
  };
}
