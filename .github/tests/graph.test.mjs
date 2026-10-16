// The committed workflow graph, resolved event by event: where macOS runs,
// what it runs there, and that every required context is still reported.

import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { before, describe, test } from "node:test";
import { condition, loadWorkflows, plan, Unresolved } from "../lib/graph.mjs";
import { isObject } from "../lib/util.mjs";
import { classified, fixture, history, outputsOf, policy, ROOT } from "./support/histories.mjs";
import { dispatchRunner, pullRequestPayload, runsFor, WORKFLOWS } from "./support/lifecycle.mjs";

const workflows = loadWorkflows(WORKFLOWS);

/** An Actions expression as a workflow writes it, without spelling its opener in a literal. */
function expression(inner) {
  return `${"$"}{{ ${inner} }}`;
}
const AGENTS = readFileSync(join(ROOT, "AGENTS.md"), "utf8");

/** The lines of one of AGENTS.md's marker blocks, as `just check-repo` reads them. */
function block(name) {
  const end = AGENTS.indexOf(`[//]: # (END ${name})`);
  assert.ok(end > 0, `AGENTS.md carries the ${name} block`);
  const start = AGENTS.lastIndexOf("\n\n", AGENTS.lastIndexOf("\n- ", end));
  return AGENTS.slice(start, end)
    .split("\n")
    .filter((line) => line.startsWith("- "));
}

const REQUIRED = block("required-checks").map((line) => line.slice(2).replaceAll("`", ""));
const PLATFORMS = block("supported-platforms").map((line) => {
  const [, id, runner] = /^- `([^`]+)` — runner `([^`]+)`/.exec(line);
  return { id, runner };
});
const MACOS = PLATFORMS.find((platform) => platform.id.startsWith("macos-"));

/** Every command a job's `run:` steps execute, in order, one per line. */
function commands(job) {
  return (job.steps ?? [])
    .filter((step) => typeof step.run === "string")
    .flatMap((step) =>
      step.run
        .split("\n")
        .map((line) => line.trim())
        .filter(Boolean),
    );
}

function macosJobs(runs) {
  return runs.flatMap(({ run }) =>
    run.jobs.filter((job) => job.macos).map((job) => ({ workflow: run.workflow, ...job })),
  );
}

function displays(runs) {
  return runs.flatMap(({ run }) => run.jobs.map((job) => job.display));
}

/** The runs a pull request of one shape starts, its classification the real classifier's. */
function pullRequest(diff, branch, results = {}) {
  const made = history(diff);
  const payload = pullRequestPayload("opened", branch, "same-repository", made);
  const classification = classified("pull_request", payload, made);
  const runs = [];
  for (const [file, workflow] of workflows) {
    if (!("pull_request" in workflow.on)) {
      continue;
    }
    runs.push({
      file,
      run: plan(workflows, file, {
        event: { name: "pull_request", action: "opened", payload },
        outputs: file === "ci.yml" ? { classify: outputsOf(classification) } : {},
        results,
      }),
    });
  }
  return { classification, runs };
}

describe("an ordinary pull request", () => {
  let ordinary;
  before(() => {
    ordinary = pullRequest("source", "ordinary");
  });

  test("runs one macOS job: the gate and the integration tier, on the platform list's macOS runner", () => {
    const mac = macosJobs(ordinary.runs);
    assert.deepEqual(
      mac.map((job) => [job.workflow, job.job, job.runner]),
      [["ci.yml", "macos", [MACOS.runner]]],
    );
  });

  test("reports every required context exactly once", () => {
    const reported = displays(ordinary.runs);
    for (const name of REQUIRED) {
      assert.equal(reported.filter((one) => one === name).length, 1, `${name} is reported once`);
    }
  });
});

describe("release automation's version/changelog-only pull request", () => {
  let release;
  before(() => {
    release = pullRequest("version", "release");
  });

  test("is classified release-only and runs no macOS job", () => {
    assert.equal(release.classification.isReleaseOnly, true);
    assert.deepEqual(macosJobs(release.runs), []);
    const ci = release.runs.find(({ file }) => file === "ci.yml").run;
    assert.ok(ci.skipped.includes("macos"));
  });

  test("still reports every required context exactly once", () => {
    const reported = displays(release.runs);
    for (const name of REQUIRED) {
      assert.equal(reported.filter((one) => one === name).length, 1, `${name} is reported once`);
    }
  });

  test("a release pull request smuggling a source change runs the macOS job", () => {
    assert.equal(macosJobs(pullRequest("mixed", "release").runs).length, 1);
  });
});

describe("a pull request whose classification failed", () => {
  test("runs the macOS lane anyway, and its carriers still report", () => {
    const failed = pullRequest("version", "release", { classify: "failure" });
    assert.equal(macosJobs(failed.runs).length, 1);
    const reported = displays(failed.runs);
    assert.ok(
      reported.includes("gate (macos-aarch64)") && reported.includes("integration (macos-aarch64)"),
    );
  });
});

describe("the carriers", () => {
  const ci = workflows.get("ci.yml");

  test("wait on the classifier and the macOS job, and report whatever they ended as", () => {
    for (const name of ["gate (macos-aarch64)", "integration (macos-aarch64)"]) {
      const [key, job] = Object.entries(ci.jobs).find(([, one]) => one.name === name);
      assert.deepEqual(job.needs, ["classify", "macos"], `${key} needs`);
      assert.equal(job.if, "always()", `${key} condition`);
      assert.ok(!job["runs-on"].startsWith("macos"), `${key} runs off macOS`);
      for (const result of ["failure", "cancelled", "skipped"]) {
        assert.equal(
          condition(
            job.if,
            { github: {}, inputs: null, needs: {}, vars: {} },
            [result, result],
            key,
          ),
          true,
          `${key} runs after a ${result}`,
        );
      }
    }
  });

  test("each judges the lane its own context names, through the committed recipe", () => {
    for (const lane of ["gate", "integration"]) {
      const job = Object.values(ci.jobs).find((one) => one.name === `${lane} (macos-aarch64)`);
      assert.deepEqual(
        commands(job),
        [`just macos-carry ${lane}`],
        `${lane} (macos-aarch64)'s command`,
      );
      const step = job.steps.find((one) => one.run === `just macos-carry ${lane}`);
      assert.equal(step.env.NEEDS, expression("toJSON(needs)"));
    }
    const justfile = readFileSync(join(ROOT, "justfile"), "utf8");
    assert.match(
      justfile,
      /\nmacos-carry LANE:\n {4}@node \.github\/macos-policy\.mjs carry --lane \{\{quote\(LANE\)\}\}\n/,
    );
  });

  test("carry the lane each names to the step of the macOS job publishing its outcome", () => {
    const outputs = ci.jobs.macos.outputs;
    assert.equal(outputs.gate, expression("steps.gate.outcome"));
    assert.equal(outputs.integration, expression("steps.integration.outcome"));
    const ids = ci.jobs.macos.steps.map((step) => step.id);
    assert.ok(ids.includes("gate") && ids.includes("integration"));
  });
});

describe("the macOS job carries what the two macOS cells ran", () => {
  const ci = workflows.get("ci.yml");

  test("every command of the gate cell, and every command of the integration cell", () => {
    const carried = new Set(commands(ci.jobs.macos));
    // The release pull request's maintenance measurement runs on linux-x86_64
    // alone; on every other cell, macOS's included, the recipe does nothing.
    const gate = commands(ci.jobs.gate).filter((command) => command !== "just budgets-release-pr");
    for (const command of [...gate, ...commands(ci.jobs.integration)]) {
      assert.ok(carried.has(command), `the macOS job runs \`${command}\``);
    }
    assert.equal(
      ci.jobs.macos.env.PRINTOBSERVER_SERVICE_JOURNEY,
      ci.jobs.gate.env.PRINTOBSERVER_SERVICE_JOURNEY,
    );
  });

  test("the gate and integration matrices are every other platform the list names", () => {
    for (const key of ["gate", "integration"]) {
      assert.deepEqual(
        ci.jobs[key].strategy.matrix.platform,
        PLATFORMS.filter((platform) => platform !== MACOS),
      );
    }
  });
});

describe("pushes and the release", () => {
  function pushed(name, released) {
    const made = history("version");
    const payload = fixture(name, made);
    const classification = classified("push", payload, made);
    return runsFor(
      workflows,
      { name: "push", ref: payload.ref, payload },
      {
        "ci.yml": { classify: outputsOf(classification) },
        "release-plz.yml": { release: { released } },
      },
    );
  }

  test("a push to main that released nothing runs no macOS job", () => {
    const runs = pushed("push.main", "");
    assert.deepEqual(runs.map(({ file }) => file).sort(), ["ci.yml", "release-plz.yml"]);
    assert.deepEqual(macosJobs(runs), []);
  });

  test("the push that releases builds every platform's artifacts, macOS in one cell, and publishes from Linux", () => {
    const runs = pushed("push.main.release-merge", "v9.9.9");
    const release = runs.find(({ file }) => file === "release-plz.yml").run;
    const artifacts = release.jobs.filter((job) => job.job === "artifacts");
    assert.deepEqual(
      artifacts.map((job) => [job.cell.platform.id, job.runner[0]]),
      PLATFORMS.map((platform) => [platform.id, platform.runner]),
    );
    assert.deepEqual(
      macosJobs(runs).map((job) => job.display),
      ["artifacts (macos-aarch64)"],
    );
    const publish = release.jobs.find((job) => job.job === "publish");
    assert.ok(publish.runner[0].startsWith("ubuntu-"));
  });

  test("only the macOS cell of a push-cut release proves what it built, before the upload", () => {
    const job = workflows.get("release-plz.yml").jobs.artifacts;
    for (const platform of PLATFORMS) {
      // A dispatch builds an existing tag's tree, which may predate the recipes.
      const dispatched = {
        github: { event_name: "workflow_dispatch" },
        inputs: null,
        needs: {},
        vars: {},
        matrix: { platform },
      };
      assert.deepEqual(
        job.steps.filter(
          (step) =>
            /^just prove-built-/.test(step.run ?? "") &&
            condition(step.if, dispatched, [], platform.id),
        ),
        [],
        `${platform.id} on a dispatch`,
      );
      const contexts = {
        github: { event_name: "push" },
        inputs: null,
        needs: {},
        vars: {},
        matrix: { platform },
      };
      const ran = job.steps
        .filter((step) => condition(step.if, contexts, [], `artifacts (${platform.id})`))
        .map((step) => step.run ?? step.uses);
      const proving = ran.filter((step) => /^just prove-built-/.test(step ?? ""));
      if (platform === MACOS) {
        assert.deepEqual(proving, ["just prove-built-artifacts", "just prove-built-service"]);
        assert.ok(
          ran.indexOf("just prove-built-service") <
            ran.findIndex((step) => /upload-artifact/.test(step)),
          "proved before the upload",
        );
      } else {
        assert.deepEqual(proving, [], `${platform.id} proves nothing extra`);
      }
    }
    const justfile = readFileSync(join(ROOT, "justfile"), "utf8");
    assert.match(
      justfile,
      /prove-built-artifacts:\n {4}@?uv run -q python -m release_artifacts prove-built /,
    );
    assert.match(
      justfile,
      /prove-built-service:\n {4}@?uv run -q python -m release_artifacts prove-service/,
    );
  });

  test("the tag and the release it publishes start nothing", () => {
    const tag = fixture("push.tag");
    assert.deepEqual(runsFor(workflows, { name: "push", ref: tag.ref, payload: tag }, {}), []);
    const published = fixture("release.published");
    assert.deepEqual(
      runsFor(workflows, { name: "release", action: "published", payload: published }, {}),
      [],
    );
  });
});

describe("the downstream proof and the schedules", () => {
  test("install-path runs no macOS cell after a release, on its schedule, or by default dispatch", () => {
    const after = fixture("workflow_run.completed");
    const runs = [
      ...runsFor(
        workflows,
        { name: "workflow_run", action: "completed", payload: after },
        {
          "install-path.yml": { resolve: { version: "9.9.9" } },
        },
      ),
      ...runsFor(
        workflows,
        { name: "schedule", payload: fixture("schedule") },
        {
          "install-path.yml": { resolve: { version: "" } },
        },
      ),
    ];
    assert.ok(runs.some(({ file }) => file === "install-path.yml"));
    assert.deepEqual(macosJobs(runs), []);
    const proved = runs.find(({ file }) => file === "install-path.yml").run;
    assert.deepEqual(
      [...new Set(proved.jobs.filter((job) => job.cell).map((job) => job.cell.platform.id))],
      PLATFORMS.filter((platform) => platform !== MACOS).map((platform) => platform.id),
    );
  });
});

describe("the manual diagnostic", () => {
  test("defaults off macOS", () => {
    const inputs = { job: "gate" };
    const run = plan(workflows, "platform-dispatch.yml", {
      event: {
        name: "workflow_dispatch",
        target: "platform-dispatch.yml",
        payload: fixture("workflow_dispatch.default"),
      },
      inputs,
      outputs: { select: { runner: dispatchRunner("gate", "linux-x86_64") } },
    });
    assert.deepEqual(
      run.jobs.filter((job) => job.macos),
      [],
    );
  });

  test("opted into macOS, runs exactly one macOS job and no matrix", () => {
    const inputs = { job: "integration", platform: MACOS.id };
    const run = plan(workflows, "platform-dispatch.yml", {
      event: {
        name: "workflow_dispatch",
        target: "platform-dispatch.yml",
        payload: fixture("workflow_dispatch.macos"),
      },
      inputs,
      outputs: { select: { runner: dispatchRunner("integration", MACOS.id) } },
    });
    assert.deepEqual(
      run.jobs.filter((job) => job.macos).map((job) => [job.display, job.runner]),
      [[`integration (${MACOS.id})`, [MACOS.runner]]],
    );
    for (const job of Object.values(workflows.get("platform-dispatch.yml").jobs)) {
      assert.equal(job.strategy, undefined);
    }
  });

  test("is started by a person alone: no workflow dispatches another", () => {
    assert.deepEqual(Object.keys(workflows.get("platform-dispatch.yml").on), ["workflow_dispatch"]);
    for (const [file, workflow] of workflows) {
      for (const [key, job] of Object.entries(workflow.jobs)) {
        for (const command of commands(job)) {
          assert.doesNotMatch(
            command,
            /gh workflow run|\/dispatches|createWorkflowDispatch|gh api .*dispatch/,
            `${file}:${key}`,
          );
        }
        for (const step of job.steps ?? []) {
          assert.ok(
            !/dispatch/i.test(String(step.uses ?? "")),
            `${file}:${key} uses no dispatching action`,
          );
        }
      }
    }
  });
});

describe("what the graph cannot resolve is an error, never zero", () => {
  function graph(files) {
    const dir = mkdtempSync(join(tmpdir(), "workflows-"));
    for (const [name, text] of Object.entries(files)) {
      writeFileSync(join(dir, name), text);
    }
    return loadWorkflows(dir);
  }
  const push = { name: "push", ref: "refs/heads/main", payload: { ref: "refs/heads/main" } };

  test("a reusable workflow of this repository is followed, its macOS jobs counted", () => {
    const called = graph({
      "caller.yml":
        "on: push\njobs:\n  build:\n    uses: ./.github/workflows/called.yml\n    with:\n      os: macos-15\n",
      "called.yml":
        "on:\n  workflow_call:\n    inputs:\n      os: {type: string}\njobs:\n  inner:\n    runs-on: " +
        expression("inputs.os") +
        "\n    steps: [{run: echo}]\n",
    });
    const run = plan(called, "caller.yml", { event: push });
    assert.deepEqual(
      run.jobs.map((job) => [job.job, job.runner, job.macos, job.via]),
      [["build/inner", ["macos-15"], true, "caller.yml:build -> called.yml"]],
    );
  });

  /** @type {[string, Record<string, string>, RegExp][]} */
  const UNRESOLVED = [
    [
      "a branch filter in glob syntax beyond `*`",
      {
        "a.yml":
          "on:\n  push:\n    branches: ['release/v[0-9]*']\njobs:\n  a:\n    runs-on: macos-15\n    steps: [{run: echo}]\n",
      },
      /uses glob syntax this graph does not resolve/,
    ],
    [
      "a reusable workflow in another repository",
      { "a.yml": "on: push\njobs:\n  x:\n    uses: other/repo/.github/workflows/w.yml@v1\n" },
      /outside this repository/,
    ],
    [
      "a matrix built by an expression",
      {
        "a.yml":
          "on: push\njobs:\n  x:\n    strategy:\n      matrix: " +
          expression("fromJSON('{}')") +
          "\n    runs-on: ubuntu-24.04\n    steps: [{run: echo}]\n",
      },
      /cannot expand/,
    ],
    [
      "an output no scenario gave",
      {
        "a.yml":
          "on: push\njobs:\n  a:\n    runs-on: ubuntu-24.04\n    steps: [{run: echo}]\n  b:\n    needs: a\n    if: needs.a.outputs.go == 'yes'\n    runs-on: macos-15\n    steps: [{run: echo}]\n",
      },
      /needs\.a\.outputs\.go/,
    ],
    [
      "a runner chosen by an output no scenario gave",
      {
        "a.yml":
          "on: push\njobs:\n  a:\n    runs-on: ubuntu-24.04\n    steps: [{run: echo}]\n  b:\n    needs: a\n    runs-on: " +
          expression("needs.a.outputs.runner") +
          "\n    steps: [{run: echo}]\n",
      },
      /needs\.a\.outputs\.runner/,
    ],
    [
      "a condition reading a secret",
      {
        "a.yml":
          "on: push\njobs:\n  a:\n    if: secrets.TOKEN != ''\n    runs-on: macos-15\n    steps: [{run: echo}]\n",
      },
      /does not parse/,
    ],
    [
      "a path filter",
      {
        "a.yml":
          "on:\n  push:\n    paths: ['src/**']\njobs:\n  a:\n    runs-on: macos-15\n    steps: [{run: echo}]\n",
      },
      /path filter/,
    ],
  ];
  for (const [why, files, pattern] of UNRESOLVED) {
    test(why, () => {
      const loaded = graph(files);
      assert.throws(
        () => runsFor(loaded, push, {}),
        (error) => error instanceof Unresolved && pattern.test(error.message),
      );
    });
  }

  test("a malformed matrix include is an error rather than an empty list", () => {
    const loaded = graph({
      "a.yml":
        "on: push\njobs:\n  a:\n    strategy:\n      matrix:\n        os: [macos-15]\n        include: macos-14\n    runs-on: ubuntu-24.04\n    steps: [{run: echo}]\n",
    });
    assert.throws(
      () => runsFor(loaded, push, {}),
      (error) =>
        error instanceof Unresolved &&
        /matrix `include` is not a list of cells/.test(error.message),
    );
  });

  test("a workflow_run filter on the triggering run's branch is applied", () => {
    const loaded = graph({
      "after.yml":
        "on:\n  workflow_run:\n    workflows: [build]\n    types: [completed]\n    branches: [main]\njobs:\n  a:\n    runs-on: macos-15\n    steps: [{run: echo}]\n",
    });
    const finished = (branch) => ({
      name: "workflow_run",
      action: "completed",
      payload: { workflow_run: { name: "build", head_branch: branch } },
    });
    assert.equal(runsFor(loaded, finished("main"), {}).length, 1);
    assert.equal(runsFor(loaded, finished("feature"), {}).length, 0);
  });

  test("a reusable workflow called from a matrix is called once per cell", () => {
    const called = graph({
      "caller.yml":
        "on: push\njobs:\n  build:\n    strategy:\n      matrix:\n        os: [macos-15, macos-14]\n    uses: ./.github/workflows/called.yml\n    with:\n      os: " +
        expression("matrix.os") +
        "\n",
      "called.yml":
        "on:\n  workflow_call:\n    inputs:\n      os: {type: string}\njobs:\n  inner:\n    runs-on: " +
        expression("inputs.os") +
        "\n    steps: [{run: echo}]\n",
    });
    const run = plan(called, "caller.yml", { event: push });
    assert.deepEqual(
      run.jobs.map((job) => job.runner[0]),
      ["macos-15", "macos-14"],
    );
  });

  test("an output the scenario does give is read", () => {
    const loaded = graph({
      "a.yml":
        "on: push\njobs:\n  a:\n    runs-on: ubuntu-24.04\n    steps: [{run: echo}]\n  b:\n    needs: a\n    if: needs.a.outputs.go == 'yes'\n    runs-on: macos-15\n    steps: [{run: echo}]\n",
    });
    assert.equal(
      plan(loaded, "a.yml", { event: push, outputs: { a: { go: "yes" } } }).jobs.filter(
        (job) => job.macos,
      ).length,
      1,
    );
    assert.equal(
      plan(loaded, "a.yml", { event: push, outputs: { a: { go: "no" } } }).jobs.filter(
        (job) => job.macos,
      ).length,
      0,
    );
  });

  test("a matrix of every cell is expanded, include and exclude applied", () => {
    const loaded = graph({
      "a.yml":
        "on: push\njobs:\n  a:\n    strategy:\n      matrix:\n        os: [macos-15, ubuntu-24.04]\n        n: [1, 2]\n        exclude: [{os: ubuntu-24.04, n: 2}]\n        include: [{os: macos-14, n: 3}]\n    runs-on: " +
        expression("matrix.os") +
        "\n    steps: [{run: echo}]\n",
    });
    const run = plan(loaded, "a.yml", { event: push });
    assert.deepEqual(
      run.jobs.map((job) => job.runner[0]),
      ["macos-15", "macos-15", "ubuntu-24.04", "macos-14"],
    );
    assert.ok(isObject(run.jobs[0].cell));
  });
});

describe("the macOS job's own steps, walked as the forge runs them", () => {
  const job = workflows.get("ci.yml").jobs.macos;

  /**
   * Each step in order, run or skipped by its own `if:` over what the steps
   * before it ended as; the steps named in `failing` fail when they run.
   *
   * @param {string[]} failing
   * @returns {{outcomes: Record<string, string>, result: string, ran: string[]}}
   */
  function walk(failing) {
    /** @type {Record<string, string>} */
    const outcomes = {};
    const ran = [];
    let failed = false;
    for (const [index, step] of job.steps.entries()) {
      const id = step.id ?? `step ${index}`;
      const steps = Object.fromEntries(
        Object.entries(outcomes).map(([name, outcome]) => [
          name,
          { outcome, conclusion: outcome, outputs: {} },
        ]),
      );
      const contexts = {
        github: { event_name: "pull_request" },
        inputs: null,
        needs: {},
        vars: {},
        steps,
      };
      const runs = condition(step.if, contexts, [failed ? "failure" : "success"], `macos: ${id}`);
      const outcome = !runs ? "skipped" : failing.includes(id) ? "failure" : "success";
      failed ||= outcome === "failure";
      outcomes[id] = outcome;
      if (runs) {
        ran.push(step.run ?? step.uses);
      }
    }
    return { outcomes, result: failed ? "failure" : "success", ran };
  }

  /** What the two carriers say of one walk, through the committed command line. */
  function carried(walked) {
    const needs = {
      classify: {
        result: "success",
        outputs: { "macos-required": "true", reason: "an ordinary pull request" },
      },
      macos: {
        result: walked.result,
        outputs: { gate: walked.outcomes.gate, integration: walked.outcomes.integration },
      },
    };
    return Object.fromEntries(
      ["gate", "integration"].map((lane) => [
        lane,
        policy(["carry", "--lane", lane], { env: { NEEDS: JSON.stringify(needs) } }).status === 0
          ? "pass"
          : "fail",
      ]),
    );
  }

  /** @type {[string[], string, string][]} */
  const STEP_FAILURES = [
    [[], "pass", "pass"],
    [["gate"], "fail", "pass"],
    [["bootstrap"], "fail", "fail"],
    [["octoprint-up"], "pass", "fail"],
    [["integration"], "pass", "fail"],
  ];
  for (const [failing, gate, integration] of STEP_FAILURES) {
    const verb = (verdict) => (verdict === "pass" ? "passes" : "fails");
    test(`with ${failing.join(", ") || "nothing"} failing, the gate context ${verb(gate)} and integration ${verb(integration)}`, () => {
      const walked = walk(failing);
      assert.deepEqual(carried(walked), { gate, integration });
      // The bring-down and the temp cleanup run whatever failed before them.
      assert.ok(walked.ran.includes("just octoprint-down"));
      assert.ok(walked.ran.includes("just runner-temp cleanup"));
      if (failing.includes("gate")) {
        assert.ok(
          walked.ran.includes("just test-integration"),
          "the integration lane runs after a failed gate",
        );
      }
    });
  }
});
