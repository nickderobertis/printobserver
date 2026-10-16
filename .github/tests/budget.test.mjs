// The two budget commands, driven as `onebudgetspec check` runs them: they
// read the recorded telemetry and nothing else, and refuse a record they
// cannot trust rather than reporting a figure off it.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, test } from "node:test";
import { SCHEMA, sourceDigest } from "../lib/telemetry.mjs";
import { cleanEnvironment, GITHUB, ROOT } from "./support/histories.mjs";

const scratch = mkdtempSync(join(tmpdir(), "macos-budget-"));
process.on("exit", () => rmSync(scratch, { recursive: true, force: true }));

/** A record of two scenarios: a pull request asking for one macOS job, and a dispatch asking for one. */
function record(overrides = {}) {
  const job = (display, macos, target) => ({
    job: display,
    display,
    runner: [macos ? "macos-15" : "ubuntu-24.04"],
    macos,
    target,
  });
  return {
    schema: SCHEMA,
    source_digest: sourceDigest(ROOT),
    scenarios: [
      {
        id: "pull_request.opened/source/same-repository/ordinary",
        event_class: "pull_request.opened",
        lifecycle_step: "1 pull request opened",
        manual: false,
        runs: [
          {
            workflow: "ci.yml",
            event: "pull_request",
            action: "opened",
            macos_jobs: 1,
            jobs: [
              job("macos", true, "macos-aarch64"),
              job("gate (linux-x86_64)", false, "linux-x86_64"),
            ],
          },
        ],
      },
      {
        id: "workflow_dispatch.opted-in/gate",
        event_class: "workflow_dispatch.opted-in",
        lifecycle_step: null,
        manual: true,
        runs: [
          {
            workflow: "platform-dispatch.yml",
            event: "workflow_dispatch",
            action: "",
            macos_jobs: 1,
            jobs: [job("gate (macos-aarch64)", true, "")],
          },
        ],
      },
    ],
    ...overrides,
  };
}

function budget(metric, written, env = {}) {
  const path = join(scratch, `${metric}-${Math.random().toString(36).slice(2)}.json`);
  if (written !== undefined) {
    writeFileSync(path, typeof written === "string" ? written : JSON.stringify(written));
  }
  return spawnSync(
    process.execPath,
    [join(GITHUB, "macos-budget.mjs"), "--metric", metric, "--telemetry", path],
    {
      cwd: ROOT,
      encoding: "utf8",
      env: cleanEnvironment(env),
    },
  );
}

describe("a current record", () => {
  test("jobs per run is the largest count any one run asked for, manual runs included, broken down", () => {
    const ran = budget("jobs-per-run", record());
    assert.equal(ran.status, 0, ran.stderr);
    assert.match(ran.stdout, /^jobs-per-run: 1\n/);
    assert.match(ran.stdout, /ci\.yml=1, platform-dispatch\.yml=1/);
    assert.match(
      ran.stdout,
      /ci\.yml on pull_request\.opened: macos \[macos-aarch64 on macos-15\]/,
    );
  });

  test("runs per change counts the lifecycle alone, naming each step", () => {
    const ran = budget("runs-per-change", record());
    assert.equal(ran.status, 0, ran.stderr);
    assert.match(ran.stdout, /^runs-per-change: 1\n/);
    assert.match(
      ran.stdout,
      /1 pull request opened: ci\.yml on pull_request\.opened \(macos on macos-aarch64\)/,
    );
    assert.doesNotMatch(ran.stdout, /platform-dispatch/);
  });

  test("under a check, writes the figure and its breakdown where the check reads them", () => {
    const result = join(scratch, "result.json");
    const ran = budget("runs-per-change", record(), { ONEBUDGETSPEC_RESULT: result });
    assert.equal(ran.status, 0, ran.stderr);
    assert.equal(ran.stdout, "");
    const written = readFileSync(result, "utf8");
    assert.match(written, /1/);
    assert.match(written, /1 pull request opened/);
  });
});

describe("a record it cannot trust", () => {
  test("is refused when there is none", () => {
    const ran = budget("jobs-per-run", undefined);
    assert.equal(ran.status, 1);
    assert.match(ran.stderr, /no telemetry at .* run `bunx nx run github-workflows:test`/);
  });

  test("is refused when it is not JSON", () => {
    assert.equal(budget("jobs-per-run", "{ nope").status, 1);
  });

  test("is refused when it is another schema", () => {
    const ran = budget("jobs-per-run", record({ schema: SCHEMA + 1 }));
    assert.equal(ran.status, 1);
    assert.match(ran.stderr, /is not schema 1 telemetry/);
  });

  test("is refused when the workflows have changed since it was recorded", () => {
    const ran = budget("runs-per-change", record({ source_digest: "sha256:0" }));
    assert.equal(ran.status, 1);
    assert.match(ran.stderr, /it is stale/);
  });

  test("an unknown metric is a usage error", () => {
    const ran = budget("queue-minutes", record());
    assert.equal(ran.status, 2);
    assert.match(ran.stderr, /usage: macos-budget\.mjs --metric jobs-per-run\|runs-per-change/);
  });
});
