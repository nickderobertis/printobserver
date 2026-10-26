// The committed workflow graph, resolved event by event: where macOS runs,
// what it runs there, and that every required context is still reported.

import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { before, describe, test } from "node:test";
import { condition, loadWorkflows, plan, Unresolved } from "../lib/graph.mjs";
import { isObject } from "../lib/util.mjs";
import { classified, fixture, history, outputsOf, ROOT } from "./support/histories.mjs";
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
  return runs.flatMap(({ file, run }) => [
    ...run.jobs.map((job) => job.display),
    // A job its own condition skips still reports its context, as a success.
    ...run.skipped
      .map((key) => workflows.get(file).jobs[key]?.name)
      .filter((name) => typeof name === "string" && !name.includes("${")),
  ]);
}

/** The jobs that run macOS's work, by the lane each runs, and the ones reporting each context. */
const MACOS_JOBS = { gate: "gate-shard-macos-aarch64", integration: "integration-macos" };
const MACOS_REPORTERS = { gate: "gate-macos", integration: "integration-macos" };

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

  test("runs three macOS jobs side by side: the gate's two shards and the integration tier", () => {
    const mac = macosJobs(ordinary.runs);
    assert.deepEqual(
      mac.map((job) => [job.workflow, job.job, job.runner]),
      [
        ["ci.yml", "gate-shard-macos-aarch64", [MACOS.runner]],
        ["ci.yml", "gate-shard-macos-aarch64", [MACOS.runner]],
        ["ci.yml", "integration-macos", [MACOS.runner]],
      ],
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
    for (const key of ["gate-shard-macos-aarch64", "gate-macos", "integration-macos"]) {
      assert.ok(ci.skipped.includes(key), `${key} is skipped`);
    }
  });

  test("still reports every required context exactly once", () => {
    const reported = displays(release.runs);
    for (const name of REQUIRED) {
      assert.equal(reported.filter((one) => one === name).length, 1, `${name} is reported once`);
    }
  });

  test("a release pull request smuggling a source change runs the macOS jobs", () => {
    assert.equal(macosJobs(pullRequest("mixed", "release").runs).length, 3);
  });
});

describe("a pull request whose classification failed", () => {
  test("runs the macOS lanes anyway, and each still reports its context", () => {
    const failed = pullRequest("version", "release", { classify: "failure" });
    assert.equal(macosJobs(failed.runs).length, 3);
    const reported = displays(failed.runs);
    assert.ok(
      reported.includes("gate (macos-aarch64)") && reported.includes("integration (macos-aarch64)"),
    );
  });
});

describe("the macOS jobs", () => {
  const ci = workflows.get("ci.yml");

  /** Whether a job runs, for one event, one classifier answer and the shards' result. */
  function runs(job, event, result, required, shards = "success") {
    return condition(
      job.if,
      {
        github: { event_name: event },
        inputs: null,
        needs: {
          classify: { result, outputs: { "macos-required": required } },
          [MACOS_JOBS.gate]: { result: shards, outputs: {} },
        },
        vars: {},
      },
      [result],
      "macos",
    );
  }

  test("run on the macOS runner, waiting on the classifier alone", () => {
    for (const key of Object.values(MACOS_JOBS)) {
      const job = ci.jobs[key];
      assert.deepEqual([job.needs].flat(), ["classify"], `${key} needs`);
      const runners = job.strategy
        ? job.strategy.matrix.platform.map((cell) => cell.runner)
        : [job["runs-on"]];
      for (const runner of runners) {
        assert.equal(runner, MACOS.runner, `${key} runs on macOS`);
      }
    }
  });

  test("run where the classifier owes the lane or could not answer, and nowhere else", () => {
    for (const key of Object.values(MACOS_JOBS)) {
      const job = ci.jobs[key];
      assert.equal(runs(job, "pull_request", "success", "true"), true, `${key}: owed`);
      assert.equal(runs(job, "pull_request", "success", "false"), false, `${key}: not owed`);
      assert.equal(runs(job, "pull_request", "failure", ""), true, `${key}: unknown`);
      assert.equal(runs(job, "push", "failure", ""), false, `${key}: a push`);
    }
  });

  test("the gate's aggregator reports it over whatever its shards ended as, unless they were skipped", () => {
    const aggregator = ci.jobs[MACOS_REPORTERS.gate];
    assert.equal(aggregator.name, "gate (macos-aarch64)");
    assert.deepEqual(aggregator.needs, [MACOS_JOBS.gate]);
    assert.ok(!aggregator["runs-on"].startsWith("macos"), "the aggregator takes no macOS slot");
    for (const shards of ["success", "failure", "cancelled"]) {
      assert.equal(runs(aggregator, "pull_request", "success", "true", shards), true, shards);
    }
    assert.equal(runs(aggregator, "pull_request", "success", "false", "skipped"), false);
    assert.equal(ci.jobs[MACOS_REPORTERS.integration].name, "integration (macos-aarch64)");
  });

  test("each context's reporting job ends with its `gate-time` budget", () => {
    for (const [lane, key] of Object.entries(MACOS_REPORTERS)) {
      const last = ci.jobs[key].steps.at(-1);
      assert.equal(last.run, "just gate-time", `${key}'s last step`);
      assert.equal(last.env.GATE_TIME_KIND, lane, `${key}'s kind`);
    }
  });
});

describe("the macOS jobs run what the two macOS cells ran", () => {
  const ci = workflows.get("ci.yml");

  test("every command of the gate cell, and every command of the integration cell", () => {
    // The release pull request's maintenance measurement runs on linux-x86_64
    // alone; on every other cell, macOS's included, the recipe does nothing.
    // The gate cell's `just check` is the shards' `just check-shard`, which
    // `just check-repo`'s `sharded-jobs` holds to running all of it, and its
    // `gate-time` budget is the aggregator's.
    const own = ["just budgets-release-pr", "just gate-time", 'just check "$(just gate-tier)"'];
    const gate = commands(ci.jobs.gate).filter((command) => !own.includes(command));
    const integration = commands(ci.jobs.integration).filter(
      (command) => command !== "just gate-time",
    );
    for (const [cell, key] of [
      [gate, MACOS_JOBS.gate],
      [integration, MACOS_JOBS.integration],
    ]) {
      const ran = new Set(commands(ci.jobs[key]));
      for (const command of cell) {
        assert.ok(ran.has(command), `${key} runs \`${command}\``);
      }
    }
    assert.ok(
      commands(ci.jobs[MACOS_JOBS.gate]).includes(
        'just check-shard "$(just gate-tier)" "$GATE_SHARD"',
      ),
    );
    assert.equal(
      ci.jobs[MACOS_JOBS.gate].env.PRINTOBSERVER_SERVICE_JOURNEY,
      ci.jobs.gate.env.PRINTOBSERVER_SERVICE_JOURNEY,
    );
    // The Windows-target lint reads the committed tree alone, and is left to the Linux cells.
    assert.equal(ci.jobs[MACOS_JOBS.gate].env.PRINTOBSERVER_HOST_INDEPENDENT, "elsewhere");
    assert.equal(ci.jobs.gate.env.PRINTOBSERVER_HOST_INDEPENDENT, undefined);
  });

  test("the gate and integration matrices are every other platform the list names", () => {
    const others = PLATFORMS.filter((platform) => platform !== MACOS);
    assert.deepEqual(ci.jobs.integration.strategy.matrix.platform, others);
    // A sharded gate's platform is carried by its shard job's cells, each on
    // that platform's runner, rather than by the gate job's matrix.
    const shards = Object.values(ci.jobs)
      .filter((job) => typeof job.name === "string" && job.name.startsWith("gate-shard ("))
      .flatMap((job) => job.strategy.matrix.platform)
      .map(({ id, runner }) => ({ id, runner }));
    const sharded = PLATFORMS.filter((platform) => shards.some(({ id }) => id === platform.id));
    assert.deepEqual(
      ci.jobs.gate.strategy.matrix.platform,
      PLATFORMS.filter((platform) => !sharded.includes(platform)),
    );
    for (const cell of shards) {
      assert.deepEqual(
        cell,
        PLATFORMS.find((platform) => platform.id === cell.id),
      );
    }
  });
});

describe("a failed integration tier", () => {
  const ci = workflows.get("ci.yml");
  // Where the tier keeps a failed instance's logs, as the environment declares it.
  const retained = /^DEFAULT_RETAIN_DIR = "([^"]+)"$/m.exec(
    readFileSync(join(ROOT, "tools", "octoprint-env", "octoprint_env.py"), "utf8"),
  )[1];

  /**
   * The steps a job runs, in order, when `just test-integration` ends as `tier`
   * and every other step that runs succeeds — each step's `if` evaluated by the
   * forge's own rule over the results of the steps before it, under the contexts
   * a pull request gives it and no `steps` outputs at all.
   */
  function walked(key, tier) {
    const contexts = {
      github: { event_name: "pull_request" },
      inputs: null,
      needs: {},
      vars: {},
      matrix: { platform: PLATFORMS[0] },
      steps: {},
    };
    const results = [];
    const ran = [];
    for (const step of ci.jobs[key].steps) {
      if (!condition(step.if, contexts, results, `${key}: ${step.run ?? step.uses}`)) {
        continue;
      }
      ran.push(step);
      results.push(step.run === "just test-integration" ? tier : "success");
    }
    return ran;
  }

  const uploads = (steps) =>
    steps.filter((step) => /^actions\/upload-artifact@/.test(step.uses ?? ""));

  for (const key of ["integration", MACOS_JOBS.integration]) {
    test(`${key} uploads the retained logs after the bring-down, and only when the tier failed`, () => {
      const failed = walked(key, "failure");
      assert.deepEqual(
        uploads(failed).map((step) => step.with.path),
        [retained],
        `${key} uploads ${retained} when the tier fails`,
      );
      const order = failed.map((step) => step.run ?? step.uses);
      assert.ok(
        order.indexOf("just octoprint-down") <
          order.findIndex((step) => /upload-artifact/.test(step)),
        `${key} uploads after the bring-down: ${order.join(", ")}`,
      );
      assert.deepEqual(uploads(walked(key, "success")), [], `${key} uploads nothing on a pass`);
    });
  }
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

describe("the macOS jobs' own steps, walked as the forge runs them", () => {
  /**
   * Each step of one job in order, run or skipped by its own `if:` over whether
   * a step before it failed; the steps whose command is in `failing` fail.
   *
   * @param {string} key
   * @param {string[]} failing
   * @returns {{result: string, ran: string[]}}
   */
  function walk(key, failing) {
    const ran = [];
    let failed = false;
    for (const [index, step] of workflows.get("ci.yml").jobs[key].steps.entries()) {
      const contexts = {
        github: { event_name: "pull_request" },
        inputs: null,
        needs: {},
        vars: {},
      };
      const runs = condition(
        step.if,
        contexts,
        [failed ? "failure" : "success"],
        `${key}: ${index}`,
      );
      if (runs) {
        ran.push(step.run ?? step.uses);
        failed ||= failing.includes(step.run);
      }
    }
    return { result: failed ? "failure" : "success", ran };
  }

  /** @type {[string, string[]][]} */
  const STEP_FAILURES = [
    [MACOS_JOBS.gate, []],
    [MACOS_JOBS.gate, ['just check-shard "$(just gate-tier)" "$GATE_SHARD"']],
    [MACOS_JOBS.gate, ["just bootstrap"]],
    [MACOS_JOBS.integration, []],
    [MACOS_JOBS.integration, ["just bootstrap"]],
    [MACOS_JOBS.integration, ["just octoprint-up"]],
    [MACOS_JOBS.integration, ["just test-integration"]],
  ];
  for (const [key, failing] of STEP_FAILURES) {
    test(`${key} with ${failing.join(", ") || "nothing"} failing ${failing.length ? "fails" : "passes"}, and cleans up`, () => {
      const walked = walk(key, failing);
      assert.equal(walked.result, failing.length ? "failure" : "success");
      // The temp cleanup, and the bring-down where there is one, run whatever failed.
      assert.ok(walked.ran.includes("just runner-temp cleanup"));
      if (key === MACOS_JOBS.integration) {
        assert.ok(walked.ran.includes("just octoprint-down"));
      }
      // The budget step, where the job carries one, reads a figure only where
      // the job's own work succeeded.
      if (key === MACOS_REPORTERS.integration) {
        assert.equal(walked.ran.includes("just gate-time"), !failing.length);
      }
    });
  }
});
