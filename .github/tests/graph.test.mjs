// The committed workflow graph, resolved event by event: which jobs each event
// starts, where macOS runs, what it runs there, and that every required context
// is still reported.

import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, test } from "node:test";
import { condition, loadWorkflows, plan, Unresolved } from "../lib/graph.mjs";
import { isObject } from "../lib/util.mjs";
import { fixture, ROOT } from "./support/histories.mjs";
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
/** The platforms `test`'s matrix carries: every one but macOS, whose cell is a job of its own. */
const MATRIXED = PLATFORMS.filter((platform) => platform !== MACOS);

/** The commits a scenario's payload names; nothing here reads a history. */
const COMMITS = { base: "a".repeat(40), head: "b".repeat(40) };

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

/** Each job instance a set of runs starts, as `file:key` and its platform where it has a cell. */
function instances(runs) {
  return runs
    .flatMap(({ file, run }) =>
      run.jobs.map((job) => {
        const platform = job.cell?.platform?.id;
        return `${file}:${job.job}${platform ? ` (${platform})` : ""}`;
      }),
    )
    .sort();
}

/** Every context a set of runs reports, a skipped job's included. */
function displays(graph, runs) {
  return runs.flatMap(({ file, run }) => [
    ...run.jobs.map((job) => job.display),
    // A job its own condition skips still reports its context, as a success.
    ...run.skipped
      .map((key) => graph.get(file).jobs[key]?.name)
      .filter((name) => typeof name === "string" && !name.includes("${")),
  ]);
}

/**
 * The runs a pull request opened from one branch convention and one ownership starts.
 *
 * @param {Map<string, import("../lib/graph.mjs").Workflow>} graph
 * @param {string} branch `ordinary` or `release`
 * @param {string} [ownership]
 */
function pullRequest(graph, branch, ownership = "same-repository") {
  const payload = pullRequestPayload("opened", branch, ownership, COMMITS);
  return runsFor(graph, { name: "pull_request", action: "opened", payload }, {});
}

/** The runs a push to `main` that released nothing starts. */
function pushToMain(graph) {
  const payload = fixture("push.main", COMMITS);
  return runsFor(
    graph,
    { name: "push", ref: payload.ref, payload },
    { "release-plz.yml": { release: { released: "" } } },
  );
}

const TESTS = MATRIXED.map((platform) => `ci.yml:test (${platform.id})`);
const WARMS = MATRIXED.map((platform) => `warm.yml:warm (${platform.id})`);
/** The job instances each event starts, outside `release-plz.yml`, whose own tests are below. */
const EXPECTED = {
  "an ordinary pull request": [
    "ci.yml:lint",
    ...TESTS,
    "ci.yml:test-macos",
    "ci.yml:skilltest",
    "ci.yml:budgets",
  ].sort(),
  "release automation's pull request": [
    "ci.yml:lint",
    ...TESTS,
    "ci.yml:skilltest",
    "ci.yml:budgets",
  ].sort(),
  "a fork's pull request from a release-plz- branch": [
    "ci.yml:lint",
    ...TESTS,
    "ci.yml:test-macos",
    "ci.yml:skilltest",
    "ci.yml:budgets",
  ].sort(),
  "a push to main": ["ci.yml:budgets", ...WARMS].sort(),
};

/** Each event the expectations name, as the runs it starts in a graph. */
function scenarios(graph) {
  return {
    "an ordinary pull request": pullRequest(graph, "ordinary"),
    "release automation's pull request": pullRequest(graph, "release"),
    "a fork's pull request from a release-plz- branch": pullRequest(graph, "release", "fork"),
    "a push to main": pushToMain(graph).filter(({ file }) => file !== "release-plz.yml"),
  };
}

/**
 * Every way a graph is not the eight-job shape: an event starting other jobs,
 * a sharded or doubled cell, a step running a split tier, a `needs` edge
 * anywhere but `budgets`, or a timeout off the hour.
 *
 * @param {Map<string, import("../lib/graph.mjs").Workflow>} graph
 * @returns {string[]}
 */
function shapeFindings(graph) {
  const found = [];
  for (const [name, runs] of Object.entries(scenarios(graph))) {
    const started = instances(runs);
    for (const one of started.filter((instance) => !EXPECTED[name].includes(instance))) {
      found.push(`${name} starts ${one}, which it should not`);
    }
    for (const one of EXPECTED[name].filter((instance) => !started.includes(instance))) {
      found.push(`${name} does not start ${one}`);
    }
    if (started.length !== EXPECTED[name].length) {
      found.push(`${name} starts ${started.length} job instances, not ${EXPECTED[name].length}`);
    }
    // A matrix's cells, as the plan expands them: one per platform, never a shard.
    const cells = new Map();
    for (const { file, run } of runs) {
      for (const job of run.jobs.filter((one) => one.cell)) {
        const key = `${file}:${job.job} (${job.cell.platform?.id})`;
        cells.set(key, (cells.get(key) ?? 0) + 1);
      }
    }
    for (const [key, count] of cells) {
      if (count > 1) {
        found.push(`${name} starts ${key} ${count} times: a platform has more than one cell`);
      }
    }
  }
  for (const file of ["ci.yml", "warm.yml"]) {
    for (const [key, job] of Object.entries(graph.get(file).jobs)) {
      if (/shard/i.test(JSON.stringify(job.strategy ?? {}))) {
        found.push(`${file}:${key}'s matrix carries a shard`);
      }
      for (const command of commands(job)) {
        if (/shard|GATE_SHARD|gate-tier|gate-time/i.test(command)) {
          found.push(`${file}:${key} runs a split tier: \`${command}\``);
        }
      }
      const needs = [job.needs ?? []].flat();
      if (needs.length && !(file === "ci.yml" && key === "budgets")) {
        found.push(`${file}:${key} needs ${needs.join(", ")}: only \`budgets\` waits on a job`);
      }
    }
  }
  const ci = graph.get("ci.yml").jobs;
  const others = Object.keys(ci).filter((key) => key !== "budgets");
  if (
    JSON.stringify([...[ci.budgets?.needs ?? []].flat()].sort()) !== JSON.stringify(others.sort())
  ) {
    found.push(`ci.yml:budgets does not need every other job (${others.join(", ")})`);
  }
  for (const [key, job] of Object.entries(ci)) {
    const minutes = job["timeout-minutes"];
    if (key === "budgets" ? !(minutes <= 10) : minutes !== 60) {
      found.push(`ci.yml:${key}'s timeout-minutes is ${minutes}`);
    }
  }
  return found;
}

/**
 * The shape findings over the committed workflows with one passage of `ci.yml` replaced.
 *
 * @param {string} old
 * @param {string} replacement
 */
function brokenShape(old, replacement) {
  const text = readFileSync(join(WORKFLOWS, "ci.yml"), "utf8");
  assert.ok(text.includes(old), `the committed ci.yml carries ${JSON.stringify(old)}`);
  const directory = mkdtempSync(join(tmpdir(), "graph-"));
  cpSync(WORKFLOWS, directory, { recursive: true });
  writeFileSync(join(directory, "ci.yml"), text.replace(old, replacement));
  return shapeFindings(loadWorkflows(directory)).join("\n");
}

describe("the jobs each event starts", () => {
  for (const [name, runs] of Object.entries(scenarios(workflows))) {
    test(`${name} starts exactly its jobs`, () => {
      assert.deepEqual(instances(runs), EXPECTED[name]);
    });
  }

  test("a pull request starts ci.yml alone", () => {
    for (const branch of ["ordinary", "release"]) {
      assert.deepEqual(
        pullRequest(workflows, branch).map(({ file }) => file),
        ["ci.yml"],
      );
    }
  });

  test("a push to main starts ci.yml's budgets, warm.yml's legs and release-plz.yml", () => {
    const runs = pushToMain(workflows);
    assert.deepEqual(runs.map(({ file }) => file).sort(), [
      "ci.yml",
      "release-plz.yml",
      "warm.yml",
    ]);
    const ci = runs.find(({ file }) => file === "ci.yml").run;
    assert.deepEqual(ci.skipped.sort(), ["lint", "skilltest", "test", "test-macos"]);
  });

  test("release automation's pull request skips the macOS job, which reports its context as passing", () => {
    const ci = pullRequest(workflows, "release").find(({ file }) => file === "ci.yml").run;
    assert.deepEqual(ci.skipped, ["test-macos"]);
    assert.equal(workflows.get("ci.yml").jobs["test-macos"].name, `test (${MACOS.id})`);
  });

  test("the committed graph is the eight-job shape", () => {
    assert.deepEqual(shapeFindings(workflows), []);
  });

  for (const [name, branch, ownership] of [
    ["an ordinary pull request", "ordinary", "same-repository"],
    ["release automation's pull request", "release", "same-repository"],
    ["a fork's pull request from a release-plz- branch", "release", "fork"],
  ]) {
    test(`${name} reports every required context exactly once`, () => {
      const reported = displays(workflows, pullRequest(workflows, branch, ownership));
      for (const required of REQUIRED) {
        assert.equal(
          reported.filter((one) => one === required).length,
          1,
          `${required} is reported once`,
        );
      }
    });
  }
});

describe("a graph off the eight-job shape is refused", () => {
  const cases = [
    [
      "an added job",
      "  budgets:\n",
      "  extra:\n    if: github.event_name == 'pull_request'\n    runs-on: ubuntu-24.04\n    timeout-minutes: 60\n    steps:\n      - run: just check\n\n  budgets:\n",
      "an ordinary pull request starts ci.yml:extra, which it should not",
    ],
    [
      "a shard matrix",
      "          - id: linux-x86_64\n            runner: ubuntu-24.04\n",
      "          - id: linux-x86_64\n            runner: ubuntu-24.04\n            shard: 1\n          - id: linux-x86_64\n            runner: ubuntu-24.04\n            shard: 2\n",
      "ci.yml:test's matrix carries a shard",
    ],
    [
      "a shard matrix, by its doubled cells",
      "          - id: linux-x86_64\n            runner: ubuntu-24.04\n",
      "          - id: linux-x86_64\n            runner: ubuntu-24.04\n            shard: 1\n          - id: linux-x86_64\n            runner: ubuntu-24.04\n            shard: 2\n",
      "starts ci.yml:test (linux-x86_64) 2 times",
    ],
    [
      "an added needs edge",
      `    name: test (${expression("matrix.platform.id")})\n`,
      `    name: test (${expression("matrix.platform.id")})\n    needs: [lint]\n`,
      "ci.yml:test needs lint: only `budgets` waits on a job",
    ],
  ];
  for (const [name, old, replacement, naming] of cases) {
    test(name, () => {
      const found = brokenShape(old, replacement);
      assert.ok(found.includes(naming), `expected a finding naming ${naming}; got:\n${found}`);
    });
  }
});

describe("the macOS job", () => {
  const ci = workflows.get("ci.yml");
  const job = ci.jobs["test-macos"];

  /** Whether the macOS job runs on a pull request from `head` in `owner`'s repository. */
  function runs(event, head, owner) {
    return condition(
      job.if,
      {
        github: {
          event_name: event,
          head_ref: head,
          repository: "nickderobertis/printobserver",
          event: { pull_request: { head: { repo: { full_name: `${owner}/printobserver` } } } },
        },
        inputs: null,
        needs: {},
        vars: {},
      },
      [],
      "test-macos",
    );
  }

  test("runs on the macOS runner, waiting on nothing", () => {
    assert.equal(job["runs-on"], MACOS.runner);
    assert.equal(job.needs, undefined);
    assert.equal(job.strategy, undefined);
  });

  test("runs on every pull request but release automation's own", () => {
    assert.equal(runs("pull_request", "feature/a-change", "nickderobertis"), true);
    assert.equal(runs("pull_request", "release-plz-2026-10-16T08-00-00Z", "nickderobertis"), false);
    assert.equal(runs("pull_request", "release-plz-2026-10-16T08-00-00Z", "contributor"), true);
    assert.equal(runs("push", "", "nickderobertis"), false);
  });

  test("runs what a test cell runs, but the release pull request's maintenance budget", () => {
    // The release pull request's maintenance measurement runs on linux-x86_64
    // alone; on every other cell, macOS's included, the recipe does nothing.
    assert.deepEqual(
      commands(job),
      commands(ci.jobs.test).filter((command) => command !== "just budgets-release-pr"),
    );
    assert.equal(
      job.env.PRINTOBSERVER_SERVICE_JOURNEY,
      ci.jobs.test.env.PRINTOBSERVER_SERVICE_JOURNEY,
    );
    assert.equal(job.env.PRINTOBSERVER_PLATFORM, MACOS.id);
  });

  test("the test and warm matrices are every other platform the list names", () => {
    assert.deepEqual(ci.jobs.test.strategy.matrix.platform, MATRIXED);
    assert.deepEqual(workflows.get("warm.yml").jobs.warm.strategy.matrix.platform, MATRIXED);
  });
});

describe("the real-print suite's job", () => {
  const job = workflows.get("ci.yml").jobs.skilltest;
  const at = job.steps.findIndex((step) => step.id === "classify");

  /** Whether one step runs, the classifier having answered `owed`, every step before it passing. */
  function runs(step, owed, hit = "false") {
    return condition(
      step.if,
      {
        github: { event_name: "pull_request" },
        inputs: null,
        needs: {},
        vars: {},
        env: { REAL_PRINTS_STORE: String(job.env.REAL_PRINTS_STORE) },
        steps: {
          classify: { outputs: { owed } },
          "real-prints-cache": { outputs: { "cache-hit": hit } },
        },
      },
      [],
      `skilltest: ${step.run ?? step.uses}`,
    );
  }

  test("decides in its own step, whose failure fails nothing", () => {
    assert.ok(at >= 0, "a step with id classify");
    assert.match(job.steps[at].run, /^just skilltest-classify /);
    assert.equal(job.steps[at]["continue-on-error"], true);
    assert.equal(job.needs, undefined);
  });

  test("runs its work where it is owed or the classifier could not answer, and skips it where not", () => {
    const work = job.steps
      .slice(at + 1)
      .filter((step) => !/REAL_PRINTS_STORE/.test(String(step.if)));
    assert.ok(work.some((step) => step.run === "just skilltest-record"));
    for (const step of work) {
      const name = step.run ?? step.uses;
      assert.equal(runs(step, "true"), true, `${name} runs where owed`);
      assert.equal(runs(step, ""), true, `${name} runs where the classifier failed`);
      assert.equal(runs(step, "false"), false, `${name} is skipped where not owed`);
    }
  });

  test("still restores the real-print store where it is not owed", () => {
    const store = job.steps
      .slice(at + 1)
      .filter((step) => /REAL_PRINTS_STORE/.test(String(step.if)));
    const ran = (owed, hit) =>
      store.filter((step) => runs(step, owed, hit)).map((step) => step.run ?? step.uses);
    for (const owed of ["true", "false", ""]) {
      assert.deepEqual(ran(owed, "false"), [
        'just real-prints-cache-key >> "$GITHUB_OUTPUT"',
        "actions/cache/restore@v4",
        "just real-prints-fetch",
        "actions/cache/save@v4",
      ]);
      assert.deepEqual(ran(owed, "true"), [
        'just real-prints-cache-key >> "$GITHUB_OUTPUT"',
        "actions/cache/restore@v4",
        "just real-prints-checkout",
      ]);
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
   * The steps a job runs, in order, when `just ci-test` — which runs the tier
   * beside its end-to-end tier — ends as `tier`
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
      results.push(step.run === "just ci-test" ? tier : "success");
    }
    return ran;
  }

  // The uploads of the retained logs; each job's coverage data is uploaded
  // on every pass, for `budgets`, and is no failure's evidence.
  const uploads = (steps) =>
    steps.filter(
      (step) => /^actions\/upload-artifact@/.test(step.uses ?? "") && step.with?.path === retained,
    );

  for (const key of ["test", "test-macos"]) {
    test(`${key} uploads the retained logs after the bring-down, and only when the tier failed`, () => {
      const failed = walked(key, "failure");
      assert.deepEqual(
        uploads(failed).map((step) => step.with.path),
        [retained],
        `${key} uploads ${retained} when the tier fails`,
      );
      const order = failed.map((step) => step.run ?? step.uses);
      assert.ok(
        order.indexOf("just octoprint-down") < failed.indexOf(uploads(failed)[0]),
        `${key} uploads after the bring-down: ${order.join(", ")}`,
      );
      assert.deepEqual(uploads(walked(key, "success")), [], `${key} uploads nothing on a pass`);
    });
  }
});

describe("pushes and the release", () => {
  function pushed(name, released) {
    const payload = fixture(name, COMMITS);
    return runsFor(
      workflows,
      { name: "push", ref: payload.ref, payload },
      { "release-plz.yml": { release: { released } } },
    );
  }

  test("a push to main that released nothing runs no macOS job", () => {
    const runs = pushed("push.main", "");
    assert.deepEqual(runs.map(({ file }) => file).sort(), [
      "ci.yml",
      "release-plz.yml",
      "warm.yml",
    ]);
    // `warm.yml` keeps no macOS leg: its `test` job compiles cold.
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
    const inputs = { job: "test" };
    const run = plan(workflows, "platform-dispatch.yml", {
      event: {
        name: "workflow_dispatch",
        target: "platform-dispatch.yml",
        payload: fixture("workflow_dispatch.default"),
      },
      inputs,
      outputs: { select: { runner: dispatchRunner("test", "linux-x86_64") } },
    });
    assert.deepEqual(
      run.jobs.filter((job) => job.macos),
      [],
    );
  });

  test("opted into macOS, runs exactly one macOS job and no matrix", () => {
    const inputs = { job: "test", platform: MACOS.id };
    const run = plan(workflows, "platform-dispatch.yml", {
      event: {
        name: "workflow_dispatch",
        target: "platform-dispatch.yml",
        payload: fixture("workflow_dispatch.macos"),
      },
      inputs,
      outputs: { select: { runner: dispatchRunner("test", MACOS.id) } },
    });
    assert.deepEqual(
      run.jobs.filter((job) => job.macos).map((job) => [job.display, job.runner]),
      [[`test (${MACOS.id})`, [MACOS.runner]]],
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

describe("the test jobs' own steps, walked as the forge runs them", () => {
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
        github: { event_name: "pull_request", ref: "refs/pull/1/merge" },
        inputs: null,
        needs: {},
        vars: {},
        steps: {},
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

  for (const key of ["test", "test-macos"]) {
    for (const failing of [[], ["just bootstrap"], ["just ci-test"], ["just octoprint-up"]]) {
      test(`${key} with ${failing.join(", ") || "nothing"} failing ${failing.length ? "fails" : "passes"}, and cleans up`, () => {
        const walked = walk(key, failing);
        assert.equal(walked.result, failing.length ? "failure" : "success");
        // The bring-down and the temp cleanup run whatever failed.
        assert.ok(walked.ran.includes("just octoprint-down"));
        assert.ok(walked.ran.includes("just runner-temp cleanup"));
      });
    }
  }
});
