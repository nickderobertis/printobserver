// Every event class one change meets, resolved against the committed workflow
// graph: the scenarios `github-workflows:test` records as telemetry.
//
// A pull request's classification is the real classifier's answer over a
// generated history of that diff class; a platform dispatch's runner is the
// real `just dispatch-resolve`'s; every other job output a condition reads is
// the scenario's, named here, and a condition reading one no scenario names
// fails the run rather than reading empty. What the release workflow answered
// it released, and which version the install-path proof resolved, are the two
// outputs a scenario states — the release program and the forge are what
// decide them, and both answers are exercised.
//
// One change's automatic life, in order: its pull request is opened; it is
// merged to `main`; release automation's version/changelog-only pull request
// is opened; that is merged and the release is cut, tagged and published; the
// install-path proof follows each release-plz run; and the weekly schedules
// fire. Dispatches are outside it: a person starts each one.

import { spawnSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { loadWorkflows, plan, triggers } from "../../lib/graph.mjs";
import { isObject } from "../../lib/util.mjs";
import {
  bumped,
  classified,
  cleanEnvironment,
  fixture,
  GITHUB,
  history,
  outputsOf,
  ROOT,
  workspaceVersion,
} from "./histories.mjs";

export const WORKFLOWS = join(GITHUB, "workflows");

/** The twelve event classes, as the telemetry names them. */
export const EVENT_CLASSES = [
  "pull_request.opened",
  "pull_request.synchronize",
  "pull_request.reopened",
  "push.base",
  "push.release-merge",
  "push.release-tag",
  "release.published",
  "workflow_run.completed",
  "repository_dispatch",
  "schedule",
  "workflow_dispatch.default",
  "workflow_dispatch.opted-in",
];

export const DIFF_CLASSES = ["source", "version", "dependency"];
export const OWNERSHIPS = ["same-repository", "fork"];
export const BRANCHES = ["ordinary", "release"];

/**
 * The runner `just dispatch-resolve` answers for one job on one platform.
 *
 * @param {string} job
 * @param {string} platform
 * @returns {string}
 */
export function dispatchRunner(job, platform) {
  const ran = spawnSync("just", ["dispatch-resolve", job, platform], {
    cwd: ROOT,
    encoding: "utf8",
    env: cleanEnvironment(),
  });
  if (ran.status !== 0) {
    throw new Error(`just dispatch-resolve ${job} ${platform} exited ${ran.status}: ${ran.stderr}`);
  }
  const line = ran.stdout.split("\n").find((one) => one.startsWith("runner="));
  if (!line) {
    throw new Error(`just dispatch-resolve ${job} ${platform} answered no runner: ${ran.stdout}`);
  }
  return line.slice("runner=".length);
}

/**
 * The pull request payload for one action, branch and ownership.
 *
 * @param {string} action
 * @param {string} branch
 * @param {string} ownership
 * @param {{base: string, head: string}} commits
 * @returns {Record<string, any>}
 */
export function pullRequestPayload(action, branch, ownership, commits) {
  const template =
    ownership === "fork"
      ? "pull_request.fork"
      : branch === "release"
        ? "pull_request.release-plz"
        : `pull_request.${action}`;
  const payload = fixture(template, commits);
  payload.action = action;
  if (branch === "ordinary") {
    payload.pull_request.head.ref = "feature/a-change";
    payload.pull_request.title = "feat: a change";
  }
  return payload;
}

/**
 * Every run one event starts, and the `workflow_run` events those runs start
 * in turn, each resolved against the graph.
 *
 * @param {Map<string, any>} workflows
 * @param {import("../../lib/graph.mjs").Event} event
 * @param {Record<string, Record<string, Record<string, string>>>} outputs by workflow file, then job
 * @returns {{file: string, run: import("../../lib/graph.mjs").PlannedRun}[]}
 */
export function runsFor(workflows, event, outputs) {
  const started = [];
  for (const [file, workflow] of workflows) {
    const crons =
      event.name === "schedule" && event.cron === undefined
        ? (workflow.on.schedule ?? []).map((entry) => entry.cron)
        : [event.cron];
    for (const cron of crons) {
      const fired = { ...event, cron };
      if (!triggers(workflow, fired)) {
        continue;
      }
      started.push({
        file,
        run: plan(workflows, file, { event: fired, outputs: outputs[file] ?? {} }),
      });
    }
  }
  return started;
}

/**
 * The `workflow_run` event a finished run starts, as the forge sends it.
 *
 * @param {string} name the finished workflow's name
 * @param {string} triggeredBy the event that started it
 * @param {string} head
 * @returns {import("../../lib/graph.mjs").Event}
 */
export function completed(name, triggeredBy, head) {
  const payload = fixture("workflow_run.completed", { head });
  payload.workflow_run.name = name;
  payload.workflow_run.event = triggeredBy;
  payload.workflow.name = name;
  return { name: "workflow_run", action: "completed", payload };
}

/**
 * @param {Map<string, any>} workflows
 * @param {string} file
 * @param {import("../../lib/graph.mjs").PlannedRun} run
 * @returns {import("../../lib/telemetry.mjs").RunRow}
 */
function row(workflows, file, run) {
  return {
    workflow: file,
    event: run.event,
    action: run.action,
    macos_jobs: run.jobs.filter((job) => job.macos).length,
    jobs: run.jobs.map((job) => ({
      job: job.job,
      display: job.display,
      runner: job.runner,
      macos: job.macos,
      target: targetOf(workflows.get(file), job),
    })),
  };
}

/**
 * The supported platform one job instance runs for: its matrix cell's, or the
 * one its own environment names.
 */
function targetOf(workflow, job) {
  if (
    isObject(job.cell) &&
    isObject(job.cell.platform) &&
    typeof job.cell.platform.id === "string"
  ) {
    return job.cell.platform.id;
  }
  const declared = workflow?.jobs?.[job.job]?.env?.PRINTOBSERVER_PLATFORM;
  return typeof declared === "string" && !declared.includes("${{") ? declared : "";
}

/**
 * Every scenario, resolved: the record `github-workflows:test` writes.
 *
 * @returns {import("../../lib/telemetry.mjs").ScenarioRow[]}
 */
export function scenarios() {
  const workflows = loadWorkflows(WORKFLOWS);
  const next = `v${bumped(workspaceVersion())}`;
  /** @type {import("../../lib/telemetry.mjs").ScenarioRow[]} */
  const rows = [];
  const add = (scenario, started) =>
    rows.push({ ...scenario, runs: started.map(({ file, run }) => row(workflows, file, run)) });
  const followed = (scenario, started, release) => {
    add(scenario, started);
    for (const { run } of started) {
      const downstream = runsFor(workflows, completed(run.name, run.event, "f".repeat(40)), {
        "install-path.yml": { resolve: { version: release } },
      });
      if (downstream.length) {
        add(
          {
            ...scenario,
            id: `${scenario.id}/workflow_run(${run.name})`,
            event_class: "workflow_run.completed",
            lifecycle_step:
              scenario.lifecycle_step && `${scenario.lifecycle_step}, then its downstream proof`,
            classification: null,
          },
          downstream,
        );
      }
    }
  };

  // Pull requests: every action, diff class, ownership and branch convention,
  // each classified by the real classifier over a history of that diff.
  const classifications = new Map();
  for (const diff of DIFF_CLASSES) {
    const made = history(diff);
    for (const ownership of OWNERSHIPS) {
      for (const branch of BRANCHES) {
        const payload = pullRequestPayload("opened", branch, ownership, made);
        classifications.set(
          `${diff}/${ownership}/${branch}`,
          classified("pull_request", payload, made),
        );
      }
    }
  }
  for (const action of ["opened", "synchronize", "reopened"]) {
    for (const diff of DIFF_CLASSES) {
      for (const ownership of OWNERSHIPS) {
        for (const branch of BRANCHES) {
          const classification = classifications.get(`${diff}/${ownership}/${branch}`);
          const payload = pullRequestPayload(action, branch, ownership, {
            base: "a".repeat(40),
            head: "b".repeat(40),
          });
          const step =
            action === "opened" && ownership === "same-repository"
              ? diff === "source" && branch === "ordinary"
                ? "1 pull request opened"
                : diff === "version" && branch === "release"
                  ? "3 release pull request opened"
                  : null
              : null;
          add(
            {
              id: `pull_request.${action}/${diff}/${ownership}/${branch}`,
              event_class: `pull_request.${action}`,
              diff,
              ownership,
              branch,
              lifecycle_step: step,
              manual: false,
              classification,
            },
            runsFor(
              workflows,
              { name: "pull_request", action, payload },
              {
                "ci.yml": { classify: outputsOf(classification) },
              },
            ),
          );
        }
      }
    }
  }

  // A pull request whose event cannot be read: the classifier owes macOS.
  const unreadable = history("source");
  const garbled = join(unreadable.dir, "garbled.event.json");
  writeFileSync(garbled, "{ this is not an event");
  const unknown = JSON.parse(
    spawnSync(
      process.execPath,
      [
        join(GITHUB, "macos-policy.mjs"),
        "classify",
        "--event",
        garbled,
        "--base",
        unreadable.base,
        "--head",
        unreadable.head,
      ],
      {
        cwd: unreadable.dir,
        encoding: "utf8",
        env: cleanEnvironment({ GITHUB_EVENT_NAME: "pull_request" }),
      },
    ).stdout,
  );
  add(
    {
      id: "pull_request.opened/unreadable-event",
      event_class: "pull_request.opened",
      diff: "source",
      ownership: "same-repository",
      branch: "ordinary",
      lifecycle_step: null,
      manual: false,
      classification: unknown,
    },
    runsFor(
      workflows,
      {
        name: "pull_request",
        action: "opened",
        payload: pullRequestPayload("opened", "ordinary", "same-repository", unreadable),
      },
      { "ci.yml": { classify: outputsOf(unknown) } },
    ),
  );

  // Pushes to `main`: the change's own merge, release automation's merge, and
  // a dependency bump; then the tag and the release the cut leaves.
  const pushes = [
    {
      id: "push.base/source",
      event_class: "push.base",
      diff: "source",
      fixture: "push.main",
      released: "",
      step: "2 merged to main",
    },
    {
      id: "push.base/dependency",
      event_class: "push.base",
      diff: "dependency",
      fixture: "push.main",
      released: "",
      step: null,
    },
    {
      id: "push.release-merge/version",
      event_class: "push.release-merge",
      diff: "version",
      fixture: "push.main.release-merge",
      released: next,
      step: "4 release pull request merged and released",
    },
  ];
  for (const push of pushes) {
    const made = history(push.diff);
    const payload = fixture(push.fixture, made);
    const classification = classified("push", payload, made);
    followed(
      {
        id: push.id,
        event_class: push.event_class,
        diff: push.diff,
        ownership: "same-repository",
        branch: "main",
        lifecycle_step: push.step,
        manual: false,
        classification,
      },
      runsFor(
        workflows,
        { name: "push", ref: payload.ref, payload },
        {
          "ci.yml": { classify: outputsOf(classification) },
          "release-plz.yml": { release: { released: push.released } },
        },
      ),
      push.released.replace(/^v/, ""),
    );
  }
  const quiet = (id, event_class, step, event) =>
    add(
      {
        id,
        event_class,
        diff: "version",
        ownership: "same-repository",
        branch: "main",
        lifecycle_step: step,
        manual: false,
        classification: null,
      },
      runsFor(workflows, event, {}),
    );
  const tag = fixture("push.tag");
  quiet("push.release-tag", "push.release-tag", "5 release tag pushed", {
    name: "push",
    ref: tag.ref,
    payload: tag,
  });
  const published = fixture("release.published");
  quiet("release.published", "release.published", "6 release published", {
    name: "release",
    action: "published",
    payload: published,
  });
  const dispatched = fixture("repository_dispatch");
  quiet("repository_dispatch", "repository_dispatch", null, {
    name: "repository_dispatch",
    action: dispatched.action,
    payload: dispatched,
  });

  // A downstream proof after a run that cut nothing, as its own event.
  add(
    {
      id: "workflow_run.completed/nothing-cut",
      event_class: "workflow_run.completed",
      diff: "source",
      ownership: "same-repository",
      branch: "main",
      lifecycle_step: null,
      manual: false,
      classification: null,
    },
    runsFor(workflows, completed("release-plz", "push", "f".repeat(40)), {
      "install-path.yml": { resolve: { version: "" } },
    }),
  );

  // Every schedule any workflow declares, each firing as its own run.
  for (const [file, workflow] of workflows) {
    for (const entry of workflow.on.schedule ?? []) {
      const payload = fixture("schedule");
      payload.schedule = entry.cron;
      add(
        {
          id: `schedule/${file}/${entry.cron}`,
          event_class: "schedule",
          diff: "none",
          ownership: "same-repository",
          branch: "main",
          lifecycle_step: "7 weekly schedules",
          manual: false,
          classification: null,
        },
        runsFor(
          workflows,
          { name: "schedule", cron: entry.cron, payload },
          { "install-path.yml": { resolve: { version: "" } } },
        ).filter(({ file: started }) => started === file),
      );
    }
  }

  // Manual dispatches: every workflow's with its defaults, and the platform
  // dispatch opted into macOS for every job it can run.
  const defaults = fixture("workflow_dispatch.default");
  const defaultRunner = dispatchRunner(defaults.inputs.job, "linux-x86_64");
  for (const [file, workflow] of workflows) {
    if (!("workflow_dispatch" in workflow.on)) {
      continue;
    }
    const inputs =
      file === "platform-dispatch.yml"
        ? { job: defaults.inputs.job }
        : file === "release-plz.yml"
          ? { tag: `v${workspaceVersion()}` }
          : {};
    add(
      {
        id: `workflow_dispatch.default/${file}`,
        event_class: "workflow_dispatch.default",
        diff: "none",
        ownership: "same-repository",
        branch: "main",
        lifecycle_step: null,
        manual: true,
        classification: null,
      },
      [
        {
          file,
          run: plan(workflows, file, {
            event: {
              name: "workflow_dispatch",
              target: file,
              ref: "refs/heads/main",
              payload: { ...defaults, inputs },
            },
            inputs,
            outputs: {
              select: { runner: defaultRunner },
              release: { released: `v${workspaceVersion()}` },
              resolve: { version: "" },
            },
          }),
        },
      ],
    );
  }
  const opted = fixture("workflow_dispatch.macos");
  const jobs = workflows.get("platform-dispatch.yml").on.workflow_dispatch.inputs.job.options;
  for (const job of jobs) {
    const inputs = { job, platform: opted.inputs.platform };
    add(
      {
        id: `workflow_dispatch.opted-in/${job}`,
        event_class: "workflow_dispatch.opted-in",
        diff: "none",
        ownership: "same-repository",
        branch: "main",
        lifecycle_step: null,
        manual: true,
        classification: null,
      },
      [
        {
          file: "platform-dispatch.yml",
          run: plan(workflows, "platform-dispatch.yml", {
            event: {
              name: "workflow_dispatch",
              target: "platform-dispatch.yml",
              ref: "refs/heads/main",
              payload: { ...opted, inputs },
            },
            inputs,
            outputs: { select: { runner: dispatchRunner(job, inputs.platform) } },
          }),
        },
      ],
    );
  }
  return rows;
}
