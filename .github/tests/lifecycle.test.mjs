// One change's whole life against the committed workflow graph, recorded as
// the telemetry the two macOS budgets read. The assertions here are routing —
// which run asks for macOS, for what — and never a cost bound: holding the
// figures to their limits is `onebudgetspec`'s alone, through `budgets.yaml`.

import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { describe, test } from "node:test";
import { check } from "@onebudgetspec/sdk";
import { digestFiles, invalid, SCHEMA, sourceDigest, TELEMETRY } from "../lib/telemetry.mjs";
import { baseFiles, GITHUB, ROOT } from "./support/histories.mjs";
import {
  BRANCHES,
  DIFF_CLASSES,
  EVENT_CLASSES,
  OWNERSHIPS,
  scenarios,
} from "./support/lifecycle.mjs";

const recorded = scenarios();
const byId = new Map(recorded.map((scenario) => [scenario.id, scenario]));

function spending(scenario) {
  return scenario.runs.flatMap((run) =>
    run.jobs.filter((job) => job.macos).map((job) => `${run.workflow}:${job.job}:${job.target}`),
  );
}

describe("the recorded workload", () => {
  test("covers every event class, diff class, ownership and branch convention", () => {
    assert.deepEqual(
      [...new Set(recorded.map((scenario) => scenario.event_class))].sort(),
      [...EVENT_CLASSES].sort(),
    );
    const pulls = recorded.filter((scenario) => scenario.event_class.startsWith("pull_request."));
    for (const diff of DIFF_CLASSES) {
      for (const ownership of OWNERSHIPS) {
        for (const branch of BRANCHES) {
          assert.ok(
            pulls.some(
              (one) => one.diff === diff && one.ownership === ownership && one.branch === branch,
            ),
            `${diff}/${ownership}/${branch}`,
          );
        }
      }
    }
  });

  test("records the telemetry, digest and all", () => {
    const record = {
      schema: SCHEMA,
      generated_by: "github-workflows:test (.github/tests/lifecycle.test.mjs)",
      source_digest: sourceDigest(ROOT),
      digest_files: digestFiles(ROOT),
      workload:
        "one ordinary pull request receiving one push, its base merge, release automation's version/changelog-only pull request and its merge, the release build, its tag and published release, every downstream workflow_run and schedule; 12 event classes x 3 diff classes x 2 ownership classes x 2 branch conventions, every release platform, both publish answers, and every manual dispatch with its default and opted into macOS",
      scenarios: recorded,
    };
    const path = join(GITHUB, TELEMETRY);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, `${JSON.stringify(record, null, 2)}\n`);

    const persisted = JSON.parse(readFileSync(path, "utf8"));
    assert.equal(invalid(persisted), "", "the record is one the budget commands read");
    assert.equal(persisted.source_digest, sourceDigest(ROOT));
    assert.equal(persisted.scenarios.length, recorded.length);
    for (const read of [...baseFiles(), "release-plz.toml", "justfile", "AGENTS.md"]) {
      assert.ok(persisted.digest_files.includes(read), `${read} is a digested input`);
    }
  });
});

describe("one change's life", () => {
  test("its pull request asks for the one combined macOS job", () => {
    assert.deepEqual(spending(byId.get("pull_request.opened/source/same-repository/ordinary")), [
      "ci.yml:macos:macos-aarch64",
    ]);
  });

  test("its merge, the release pull request, the tag, the release and the proofs after them ask for none", () => {
    for (const id of [
      "push.base/source",
      "push.base/source/workflow_run(release-plz)",
      "pull_request.opened/version/same-repository/release",
      "push.release-tag",
      "release.published",
      "push.release-merge/version/workflow_run(release-plz)",
      "workflow_run.completed/nothing-cut",
    ]) {
      assert.deepEqual(spending(byId.get(id)), [], id);
    }
    for (const scenario of recorded.filter((one) => one.event_class === "schedule")) {
      assert.deepEqual(spending(scenario), [], scenario.id);
    }
  });

  test("the release build asks for its macOS cell beside every other platform's", () => {
    const release = byId.get("push.release-merge/version");
    assert.deepEqual(spending(release), ["release-plz.yml:artifacts:macos-aarch64"]);
    const built = release.runs
      .find((run) => run.workflow === "release-plz.yml")
      .jobs.filter((job) => job.job === "artifacts")
      .map((job) => job.target);
    assert.deepEqual(built, [
      "linux-x86_64",
      "linux-aarch64",
      "macos-aarch64",
      "windows-x86_64",
      "windows-aarch64",
    ]);
  });

  test("a push that released nothing builds nothing", () => {
    const quiet = byId
      .get("push.base/source")
      .runs.find((run) => run.workflow === "release-plz.yml");
    assert.deepEqual(
      quiet.jobs.filter((job) => job.job === "artifacts" || job.job === "publish"),
      [],
    );
  });

  test("every macOS-spending lifecycle run is one of those two", () => {
    const lifecycle = recorded.filter((scenario) => scenario.lifecycle_step && !scenario.manual);
    assert.deepEqual(
      lifecycle.filter((scenario) => spending(scenario).length).map((scenario) => scenario.id),
      ["pull_request.opened/source/same-repository/ordinary", "push.release-merge/version"],
    );
  });
});

describe("the two budgets, through onebudgetspec", () => {
  test("measure both figures from the recorded telemetry, each with its breakdown", async () => {
    const report = await check({ paths: [join(GITHUB, "budgets.yaml")], cwd: ROOT });
    const results = new Map(report.results.map((result) => [result.id, result]));
    for (const id of ["printobserver-macos-jobs-per-run", "printobserver-macos-runs-per-change"]) {
      const result = results.get(id);
      assert.ok(result, `${id} is registered`);
      assert.notEqual(result.verdict, "error", `${id}: ${result.error}`);
      assert.equal(typeof result.actual, "number");
    }
    assert.match(
      results.get("printobserver-macos-jobs-per-run").detail,
      /ci\.yml on pull_request\.opened: macos-aarch64 \(gate, integration\)/,
    );
    assert.match(
      results.get("printobserver-macos-runs-per-change").detail,
      /4 release pull request merged and released: release-plz\.yml on push/,
    );
  });
});
