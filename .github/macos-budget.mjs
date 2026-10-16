#!/usr/bin/env node
// One macOS scheduling figure, read off the telemetry `github-workflows:test`
// recorded and reported through onebudgetspec.
//
//   macos-budget.mjs --metric jobs-per-run|runs-per-change --telemetry <path>
//
// It runs no scenario and reads no threshold: `onebudgetspec check` holds the
// figure to `budgets.yaml`. A record that is missing, of another shape, or
// computed from files that have since changed is refused, naming the target
// that writes a current one. Under `onebudgetspec check` the figure and its
// breakdown go to the file the check names; run by hand they are printed.

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { report } from "@onebudgetspec/sdk";
import { invalid, jobsPerRun, runsPerChange, sourceDigest } from "./lib/telemetry.mjs";
import { message } from "./lib/util.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const METRICS = { "jobs-per-run": jobsPerRun, "runs-per-change": runsPerChange };
const RECORD = "run `bunx nx run github-workflows:test` to record a current one";

/**
 * @param {string[]} argv
 * @returns {number}
 */
export function main(argv) {
  /** @type {Record<string, string>} */
  const options = {};
  for (let at = 0; at < argv.length; at += 2) {
    const [name, value] = [argv[at], argv[at + 1]];
    if (!["--metric", "--telemetry"].includes(name) || value === undefined || name in options) {
      return usage(`\`${name}\` is not an option given once with a value`);
    }
    options[name] = value;
  }
  const named = options["--metric"];
  const path = options["--telemetry"];
  if (!named || !Object.hasOwn(METRICS, named) || !path) {
    return usage(named ? `\`${named}\` is not a metric` : "no metric and telemetry given");
  }
  const metric = METRICS[named];
  let record;
  try {
    record = JSON.parse(readFileSync(path, "utf8"));
  } catch (unreadable) {
    process.stderr.write(
      `macos-budget: no telemetry at ${path} (${message(unreadable)}); ${RECORD}\n`,
    );
    return 1;
  }
  const refused = invalid(record);
  if (refused) {
    process.stderr.write(`macos-budget: ${path} is not a record to read: ${refused}; ${RECORD}\n`);
    return 1;
  }
  const current = sourceDigest(ROOT);
  if (record.source_digest !== current) {
    process.stderr.write(
      `macos-budget: ${path} was recorded from ${record.source_digest}, and the workflows, policy and scenarios are now ${current}: it is stale; ${RECORD}\n`,
    );
    return 1;
  }
  const figure = metric(record);
  if (!report(figure.value, figure.detail)) {
    process.stdout.write(`${named}: ${figure.value}\n${figure.detail}\n`);
  }
  return 0;
}

/**
 * @param {string} why
 * @returns {number}
 */
function usage(why) {
  process.stderr.write(
    `macos-budget: ${why}\nusage: macos-budget.mjs --metric ${Object.keys(METRICS).join("|")} --telemetry <path>\n`,
  );
  return 2;
}

process.exitCode = main(process.argv.slice(2));
