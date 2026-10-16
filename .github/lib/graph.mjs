// The committed workflow graph, resolved for one event: which workflows it
// starts, which of their jobs run, on which runner, once per matrix cell.
//
// Everything is read off the workflow files themselves. A job condition, a
// `runs-on` and a matrix-interpolated name are evaluated with GitHub's own
// published expression library (`@actions/expressions`), over the contexts the
// event and the scenario supply. What the graph cannot resolve is an error
// naming it — an output nobody modelled, a matrix built by an expression, a
// reusable workflow in another repository, a path filter — never a job counted
// as zero.
//
// What a job's steps do is not modelled: a job that runs is assumed to succeed
// unless the scenario says otherwise, and its outputs are whatever the scenario
// hands it. That is the scheduling question — how many runners of each kind one
// event requests — and nothing more.

import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { data, Evaluator, Lexer, Parser } from "@actions/expressions";
import { truthy } from "@actions/expressions/result";
import { parse as parseYaml } from "yaml";
import { isObject } from "./util.mjs";

/** A part of the graph this cannot resolve, named rather than counted as nothing. */
export class Unresolved extends Error {
  constructor(message) {
    super(message);
    this.name = "Unresolved";
  }
}

/** A runner label that is a macOS runner. */
export function isMacosLabel(label) {
  return /^macos(-|$)/i.test(label) || /(^|-)macos(-|$)/i.test(label);
}

/**
 * Every workflow in a directory, keyed by file name.
 *
 * @param {string} directory
 * @returns {Map<string, Workflow>}
 */
export function loadWorkflows(directory) {
  const found = new Map();
  for (const file of readdirSync(directory).sort()) {
    if (!/\.ya?ml$/.test(file)) {
      continue;
    }
    found.set(file, readWorkflow(directory, file));
  }
  return found;
}

/**
 * @typedef {{file: string, name: string, on: Record<string, any>, jobs: Record<string, any>, directory: string}} Workflow
 */

/**
 * @param {string} directory
 * @param {string} file
 * @returns {Workflow}
 */
export function readWorkflow(directory, file) {
  const parsed = parseYaml(readFileSync(join(directory, file), "utf8"));
  if (!isObject(parsed) || !isObject(parsed.jobs)) {
    throw new Unresolved(`${file} is not a workflow with jobs`);
  }
  return {
    file,
    directory,
    name: typeof parsed.name === "string" ? parsed.name : file,
    on: triggersOf(parsed.on ?? parsed[true], file),
    jobs: parsed.jobs,
  };
}

/**
 * The `on:` block, normalised to one entry per event.
 *
 * @param {unknown} on
 * @param {string} file
 * @returns {Record<string, any>}
 */
function triggersOf(on, file) {
  if (typeof on === "string") {
    return { [on]: {} };
  }
  if (Array.isArray(on)) {
    return Object.fromEntries(on.map((event) => [event, {}]));
  }
  if (isObject(on)) {
    return Object.fromEntries(Object.entries(on).map(([event, filter]) => [event, filter ?? {}]));
  }
  throw new Unresolved(`${file} declares no readable \`on:\``);
}

/**
 * @typedef {object} Event
 * @property {string} name the forge's event name
 * @property {string} [action] the activity type, where the event has one
 * @property {Record<string, any>} payload the event payload, as the forge hands it
 * @property {string} [ref] the ref the run is on
 * @property {string} [target] the workflow file a dispatch names
 * @property {string} [cron] the schedule entry that fired
 */

/**
 * Whether one workflow starts a run for one event, by its own `on:` filters.
 *
 * @param {Workflow} workflow
 * @param {Event} event
 * @returns {boolean}
 */
export function triggers(workflow, event) {
  const filter = workflow.on[event.name];
  if (filter === undefined) {
    return false;
  }
  for (const unmodelled of ["paths", "paths-ignore"]) {
    if (isObject(filter) && unmodelled in filter) {
      throw new Unresolved(
        `${workflow.file}: \`on.${event.name}.${unmodelled}\` is a path filter this graph does not resolve`,
      );
    }
  }
  switch (event.name) {
    case "pull_request":
    case "pull_request_target":
      return (
        typed(filter, event.action, ["opened", "synchronize", "reopened"]) &&
        branched(filter, String(event.payload?.pull_request?.base?.ref ?? ""), "branches")
      );
    case "push":
      return pushed(filter, event.ref ?? String(event.payload?.ref ?? ""));
    case "workflow_run":
      return (
        typed(filter, event.action, ["completed", "requested"]) &&
        Array.isArray(filter.workflows) &&
        filter.workflows.includes(String(event.payload?.workflow_run?.name ?? ""))
      );
    case "release":
    case "repository_dispatch":
      return typed(filter, event.action ?? event.payload?.action, null);
    case "schedule":
      return (
        Array.isArray(filter) &&
        filter.some((entry) => isObject(entry) && entry.cron === event.cron)
      );
    case "workflow_dispatch":
      return event.target === workflow.file;
    default:
      throw new Unresolved(
        `${workflow.file}: the \`${event.name}\` trigger is not one this graph resolves`,
      );
  }
}

/**
 * @param {any} filter
 * @param {string | undefined} action
 * @param {string[] | null} defaults the types the forge applies when none are named; null for every one
 * @returns {boolean}
 */
function typed(filter, action, defaults) {
  const types = isObject(filter) && Array.isArray(filter.types) ? filter.types : defaults;
  return types === null || (action !== undefined && types.includes(action));
}

/**
 * @param {any} filter
 * @param {string} name
 * @param {string} key
 * @returns {boolean}
 */
function branched(filter, name, key) {
  if (!isObject(filter)) {
    return true;
  }
  if (Array.isArray(filter[key]) && !filter[key].some((pattern) => globbed(pattern, name))) {
    return false;
  }
  const ignored = filter[`${key}-ignore`];
  return !(Array.isArray(ignored) && ignored.some((pattern) => globbed(pattern, name)));
}

/**
 * A push's ref against `branches`/`tags` filters, as the forge reads them: a
 * workflow naming only branches starts nothing for a tag, and the reverse.
 *
 * @param {any} filter
 * @param {string} ref
 * @returns {boolean}
 */
function pushed(filter, ref) {
  const tag = ref.startsWith("refs/tags/");
  const name = ref.replace(/^refs\/(heads|tags)\//, "");
  if (!isObject(filter)) {
    return true;
  }
  const branchKeys = ["branches", "branches-ignore"].some((key) => key in filter);
  const tagKeys = ["tags", "tags-ignore"].some((key) => key in filter);
  if (tag) {
    return tagKeys ? branched(filter, name, "tags") : !branchKeys;
  }
  return branchKeys ? branched(filter, name, "branches") : !tagKeys;
}

/**
 * The forge's filter glob: `*` within a path segment, `**` across them.
 *
 * @param {string} pattern
 * @param {string} name
 * @returns {boolean}
 */
function globbed(pattern, name) {
  const source = String(pattern)
    .split(/(\*\*|\*)/)
    .map((part) =>
      part === "**" ? ".*" : part === "*" ? "[^/]*" : part.replace(/[.+?^${}()|[\]\\]/g, "\\$&"),
    )
    .join("");
  return new RegExp(`^${source}$`).test(name);
}

/**
 * @typedef {object} Scenario
 * @property {Event} event
 * @property {Record<string, any>} [github] contexts beside the event's own: `ref`, `sha`, `repository`
 * @property {Record<string, string>} [inputs] dispatch inputs, over each input's declared default
 * @property {Record<string, Record<string, string>>} [outputs] each job's outputs, by job key
 * @property {Record<string, string>} [results] a job's result where it is not `success`
 */

/**
 * @typedef {{job: string, display: string, runner: string[], macos: boolean, cell: Record<string, any> | null, via: string}} PlannedJob
 * @typedef {{workflow: string, name: string, event: string, action: string, jobs: PlannedJob[], skipped: string[]}} PlannedRun
 */

/**
 * The run one workflow makes for one scenario: every job instance that runs.
 *
 * @param {Map<string, Workflow>} workflows
 * @param {string} file
 * @param {Scenario} scenario
 * @returns {PlannedRun}
 */
export function plan(workflows, file, scenario) {
  const workflow = workflows.get(file);
  if (!workflow) {
    throw new Unresolved(`no workflow ${file}`);
  }
  const github = githubContext(workflow, scenario);
  // The forge's `inputs` is a dispatch's or a call's; on any other event it
  // holds nothing, and a read of it answers null.
  const inputs = scenario.event.name === "workflow_dispatch" ? inputsOf(workflow, scenario) : null;
  const planned = planJobs(workflows, workflow, scenario, github, inputs, "");
  return {
    workflow: file,
    name: workflow.name,
    event: scenario.event.name,
    action: scenario.event.action ?? "",
    jobs: planned.jobs,
    skipped: planned.skipped,
  };
}

/**
 * @param {Workflow} workflow
 * @param {Scenario} scenario
 * @returns {Record<string, any>}
 */
function githubContext(workflow, scenario) {
  const payload = scenario.event.payload ?? {};
  const pr = isObject(payload.pull_request) ? payload.pull_request : undefined;
  return {
    event_name: scenario.event.name,
    event: payload,
    ref: scenario.event.ref ?? payload.ref ?? (pr ? `refs/pull/${pr.number}/merge` : ""),
    head_ref: pr?.head?.ref ?? "",
    base_ref: pr?.base?.ref ?? "",
    sha: payload.after ?? pr?.head?.sha ?? "",
    repository: payload.repository?.full_name ?? "",
    workflow: workflow.name,
    token: "***",
    ...(scenario.github ?? {}),
  };
}

/**
 * @param {Workflow} workflow
 * @param {Scenario} scenario
 * @returns {Record<string, string>}
 */
function inputsOf(workflow, scenario) {
  const declared =
    scenario.event.name === "workflow_dispatch" && isObject(workflow.on.workflow_dispatch?.inputs)
      ? workflow.on.workflow_dispatch.inputs
      : {};
  const inputs = {};
  for (const [name, spec] of Object.entries(declared)) {
    if (isObject(spec) && spec.default !== undefined) {
      inputs[name] = String(spec.default);
    }
  }
  for (const [name, value] of Object.entries(scenario.inputs ?? {})) {
    if (scenario.event.name === "workflow_dispatch" && !(name in declared)) {
      throw new Unresolved(`${workflow.file} declares no dispatch input \`${name}\``);
    }
    inputs[name] = value;
  }
  if (scenario.event.name === "workflow_dispatch") {
    for (const [name, spec] of Object.entries(declared)) {
      if (isObject(spec) && spec.required === true && !(name in inputs)) {
        throw new Unresolved(
          `${workflow.file}'s dispatch requires \`${name}\`, which no scenario gave`,
        );
      }
    }
  }
  return inputs;
}

/**
 * @param {Map<string, Workflow>} workflows
 * @param {Workflow} workflow
 * @param {Scenario} scenario
 * @param {Record<string, any>} github
 * @param {Record<string, string>} inputs
 * @param {string} prefix the calling job's key, for a reusable workflow's jobs
 * @returns {{jobs: PlannedJob[], skipped: string[]}}
 */
function planJobs(workflows, workflow, scenario, github, inputs, prefix) {
  /** @type {Map<string, {result: string, outputs: Record<string, string> | null}>} */
  const done = new Map();
  /** @type {PlannedJob[]} */
  const jobs = [];
  const skipped = [];
  for (const key of ordered(workflow)) {
    const job = workflow.jobs[key];
    const qualified = prefix ? `${prefix}/${key}` : key;
    const needs = needsOf(job);
    const needsContext = {};
    for (const needed of needs) {
      const result = done.get(needed);
      if (!result) {
        throw new Unresolved(
          `${workflow.file}: job \`${key}\` needs \`${needed}\`, which it does not declare`,
        );
      }
      needsContext[needed] = result;
    }
    const contexts = { github, inputs, needs: needsContext, vars: {} };
    const runs = condition(
      job.if,
      contexts,
      needs.map((needed) => needsContext[needed].result),
      `${workflow.file}: job \`${key}\``,
    );
    if (!runs) {
      done.set(key, { result: "skipped", outputs: null });
      skipped.push(qualified);
      continue;
    }
    if (typeof job.uses === "string") {
      const called = calledWorkflow(workflows, workflow, key, job.uses);
      const withInputs = Object.fromEntries(
        Object.entries(isObject(job.with) ? job.with : {}).map(([name, value]) => [
          name,
          String(
            template(String(value), contexts, `${workflow.file}: job \`${key}\`.with.${name}`),
          ),
        ]),
      );
      const nested = planJobs(workflows, called, scenario, github, withInputs, qualified);
      jobs.push(
        ...nested.jobs.map((one) => ({ ...one, via: `${workflow.file}:${key} -> ${called.file}` })),
      );
      skipped.push(...nested.skipped);
    } else {
      for (const cell of cellsOf(job, `${workflow.file}: job \`${key}\``)) {
        const cellContexts = { ...contexts, matrix: cell ?? {} };
        const runner = runnerOf(job, cellContexts, `${workflow.file}: job \`${key}\``);
        jobs.push({
          job: qualified,
          display: displayOf(job, key, cell, cellContexts, `${workflow.file}: job \`${key}\``),
          runner,
          macos: runner.some(isMacosLabel),
          cell,
          via: workflow.file,
        });
      }
    }
    // A job that ran answers exactly the outputs the scenario gives it, and a
    // read of any other is unresolved; a skipped one answers empty, as the
    // forge's does.
    const result = scenario.results?.[qualified] ?? "success";
    done.set(key, { result, outputs: scenario.outputs?.[qualified] ?? {} });
  }
  return { jobs, skipped };
}

/**
 * The jobs of a workflow in an order every job comes after the jobs it needs.
 *
 * @param {Workflow} workflow
 * @returns {string[]}
 */
function ordered(workflow) {
  const order = [];
  const state = new Map();
  const visit = (key, trail) => {
    if (state.get(key) === "done") {
      return;
    }
    if (state.get(key) === "visiting") {
      throw new Unresolved(
        `${workflow.file}: jobs need each other in a cycle (${[...trail, key].join(" -> ")})`,
      );
    }
    state.set(key, "visiting");
    for (const needed of needsOf(workflow.jobs[key] ?? {})) {
      if (!(needed in workflow.jobs)) {
        throw new Unresolved(
          `${workflow.file}: job \`${key}\` needs \`${needed}\`, which it does not declare`,
        );
      }
      visit(needed, [...trail, key]);
    }
    state.set(key, "done");
    order.push(key);
  };
  for (const key of Object.keys(workflow.jobs)) {
    visit(key, []);
  }
  return order;
}

/**
 * @param {any} job
 * @returns {string[]}
 */
function needsOf(job) {
  if (typeof job.needs === "string") {
    return [job.needs];
  }
  return Array.isArray(job.needs) ? job.needs.map(String) : [];
}

/**
 * @param {Map<string, Workflow>} workflows
 * @param {Workflow} caller
 * @param {string} key
 * @param {string} uses
 * @returns {Workflow}
 */
function calledWorkflow(workflows, caller, key, uses) {
  const local = /^\.\/\.github\/workflows\/([^/@]+\.ya?ml)$/.exec(uses);
  if (!local) {
    throw new Unresolved(
      `${caller.file}: job \`${key}\` calls \`${uses}\`, a reusable workflow outside this repository's own workflows, which this graph does not resolve`,
    );
  }
  const called = workflows.get(local[1]);
  if (!called) {
    throw new Unresolved(`${caller.file}: job \`${key}\` calls \`${uses}\`, which does not exist`);
  }
  if (!("workflow_call" in called.on)) {
    throw new Unresolved(
      `${caller.file}: job \`${key}\` calls ${called.file}, which declares no \`workflow_call\``,
    );
  }
  return called;
}

/**
 * Every cell of a job's matrix, or `[null]` for a job with none.
 *
 * @param {any} job
 * @param {string} where
 * @returns {(Record<string, any> | null)[]}
 */
function cellsOf(job, where) {
  const matrix = isObject(job.strategy) ? job.strategy.matrix : undefined;
  if (matrix === undefined) {
    return [null];
  }
  if (!isObject(matrix)) {
    throw new Unresolved(
      `${where}'s matrix is ${JSON.stringify(matrix)}, which this graph cannot expand`,
    );
  }
  if (JSON.stringify(matrix).includes("${{")) {
    throw new Unresolved(
      `${where}'s matrix is built by an expression, which this graph cannot expand`,
    );
  }
  const axes = Object.entries(matrix).filter(([name]) => name !== "include" && name !== "exclude");
  let cells = [{}];
  for (const [name, values] of axes) {
    if (!Array.isArray(values)) {
      throw new Unresolved(`${where}'s matrix axis \`${name}\` is not a list`);
    }
    cells = cells.flatMap((cell) => values.map((value) => ({ ...cell, [name]: value })));
  }
  const excluded = Array.isArray(matrix.exclude) ? matrix.exclude : [];
  cells = cells.filter((cell) => !excluded.some((pattern) => matches(cell, pattern)));
  for (const extra of Array.isArray(matrix.include) ? matrix.include : []) {
    const extended = cells.filter((cell) => axes.length && matchesOriginal(cell, extra, axes));
    if (extended.length) {
      for (const cell of extended) {
        Object.assign(cell, extra);
      }
    } else {
      cells.push({ ...extra });
    }
  }
  if (axes.length === 0 && cells.length > 1) {
    cells = cells.filter((cell) => Object.keys(cell).length);
  }
  return cells;
}

/**
 * @param {Record<string, any>} cell
 * @param {Record<string, any>} pattern
 * @returns {boolean}
 */
function matches(cell, pattern) {
  return Object.entries(pattern).every(
    ([key, value]) => JSON.stringify(cell[key]) === JSON.stringify(value),
  );
}

/**
 * @param {Record<string, any>} cell
 * @param {Record<string, any>} extra
 * @param {[string, unknown][]} axes
 * @returns {boolean}
 */
function matchesOriginal(cell, extra, axes) {
  return axes.every(
    ([name]) => !(name in extra) || JSON.stringify(cell[name]) === JSON.stringify(extra[name]),
  );
}

/**
 * @param {any} job
 * @param {Record<string, any>} contexts
 * @param {string} where
 * @returns {string[]}
 */
function runnerOf(job, contexts, where) {
  const declared = job["runs-on"];
  let labels;
  if (typeof declared === "string") {
    labels = [declared];
  } else if (Array.isArray(declared)) {
    labels = declared;
  } else if (isObject(declared) && declared.labels !== undefined) {
    labels = Array.isArray(declared.labels) ? declared.labels : [declared.labels];
  } else {
    throw new Unresolved(`${where} declares no \`runs-on\` this graph can read`);
  }
  const resolved = labels.map((label) =>
    String(template(String(label), contexts, `${where}.runs-on`)),
  );
  if (resolved.some((label) => label === "" || label.includes("${{"))) {
    throw new Unresolved(
      `${where}'s \`runs-on\` resolved to ${JSON.stringify(resolved)}, which names no runner`,
    );
  }
  return resolved;
}

/**
 * The check-run name one job instance reports under.
 *
 * @param {any} job
 * @param {string} key
 * @param {Record<string, any> | null} cell
 * @param {Record<string, any>} contexts
 * @param {string} where
 * @returns {string}
 */
function displayOf(job, key, cell, contexts, where) {
  const name = typeof job.name === "string" ? job.name : key;
  if (name.includes("${{")) {
    return String(template(name, contexts, `${where}.name`));
  }
  if (cell && Object.keys(cell).length) {
    const values = Object.values(cell).flatMap((value) =>
      isObject(value) ? Object.values(value) : [value],
    );
    return `${name} (${values.join(", ")})`;
  }
  return name;
}

/** The status functions a job condition may call, as the forge defines them. */
const STATUS = ["success", "failure", "cancelled", "always"];

/**
 * Whether a job runs, by its `if:` over what its needed jobs ended as.
 *
 * @param {unknown} declared
 * @param {Record<string, any>} contexts
 * @param {string[]} results
 * @param {string} where
 * @returns {boolean}
 */
export function condition(declared, contexts, results, where) {
  let text = declared === undefined || declared === null ? "" : String(declared).trim();
  const whole = /^\$\{\{([\s\S]*)\}\}$/.exec(text);
  if (whole) {
    text = whole[1].trim();
  }
  if (typeof declared === "boolean") {
    text = String(declared);
  }
  const callsStatus = new RegExp(`\\b(${STATUS.join("|")})\\s*\\(`, "i").test(text);
  const expression = text === "" ? "success()" : callsStatus ? text : `success() && (${text})`;
  const functions = new Map([
    ["success", status(() => results.every((result) => result === "success"))],
    ["failure", status(() => results.some((result) => result === "failure"))],
    ["cancelled", status(() => false)],
    ["always", status(() => true)],
  ]);
  return truthy(evaluate(expression, contexts, where, functions));
}

/**
 * @param {() => boolean} answer
 * @returns {{name: string, minArgs: number, maxArgs: number, call: () => any}}
 */
function status(answer) {
  return { name: "", minArgs: 0, maxArgs: 0, call: () => new data.BooleanData(answer()) };
}

/**
 * A string with every `${{ }}` in it evaluated, or the one value an expression
 * that is the whole string evaluates to.
 *
 * @param {string} text
 * @param {Record<string, any>} contexts
 * @param {string} where
 * @returns {unknown}
 */
export function template(text, contexts, where) {
  const whole = /^\$\{\{([\s\S]*?)\}\}$/.exec(text.trim());
  if (whole && !whole[1].includes("}}")) {
    return plain(evaluate(whole[1].trim(), contexts, where));
  }
  return text.replace(/\$\{\{([\s\S]*?)\}\}/g, (_, inner) =>
    String(plain(evaluate(inner.trim(), contexts, where)) ?? ""),
  );
}

/**
 * @param {any} value
 * @returns {unknown}
 */
function plain(value) {
  return value === undefined ? undefined : JSON.parse(JSON.stringify(value, data.replacer));
}

/**
 * One expression, evaluated by the published library over strict contexts.
 *
 * @param {string} expression
 * @param {Record<string, any>} contexts
 * @param {string} where
 * @param {Map<string, any>} [functions]
 * @returns {any}
 */
function evaluate(expression, contexts, where, functions = new Map()) {
  const lexed = new Lexer(expression).lex();
  const info = [...functions.keys()].map((name) => ({ name, minArgs: 0, maxArgs: 0 }));
  let parsed;
  try {
    parsed = new Parser(lexed.tokens, Object.keys(contexts), info).parse();
  } catch (refused) {
    throw new Unresolved(`${where}: \`${expression}\` does not parse: ${refused.message}`);
  }
  const named = new Map([...functions].map(([name, fn]) => [name, { ...fn, name }]));
  try {
    return new Evaluator(parsed, dictionary(contexts, "", true), named).evaluate();
  } catch (failed) {
    if (failed instanceof Unresolved) {
      throw new Unresolved(
        `${where}: \`${expression}\` reads ${failed.message}, which the scenario does not resolve`,
      );
    }
    throw failed;
  }
}

/** The contexts whose every member must be known: anything else reads `null` as the forge would. */
const STRICT = new Set([
  "needs",
  "matrix",
  "inputs",
  "secrets",
  "vars",
  "steps",
  "runner",
  "jobs",
  "strategy",
]);

/** A dictionary that refuses a member it does not hold rather than answering `null`. */
class StrictDictionary extends data.Dictionary {
  constructor(path) {
    super();
    this.path = path;
  }

  get(key) {
    const found = super.get(key);
    if (found === undefined) {
      throw new Unresolved(`\`${this.path}.${key}\``);
    }
    return found;
  }
}

/**
 * Plain JSON as the library's data, strict where the forge's context is closed.
 *
 * @param {any} value
 * @param {string} path
 * @param {boolean} top
 * @returns {any}
 */
function dictionary(value, path, top = false) {
  if (isObject(value)) {
    const strict = !top && [...STRICT].some((root) => path === root || path.startsWith(`${root}.`));
    const made = strict ? new StrictDictionary(path) : new data.Dictionary();
    for (const [key, member] of Object.entries(value)) {
      made.add(key, dictionary(member, path ? `${path}.${key}` : key));
    }
    return made;
  }
  if (Array.isArray(value)) {
    const made = new data.Array();
    for (const member of value) {
      made.add(dictionary(member, path));
    }
    return made;
  }
  if (typeof value === "string") {
    return new data.StringData(value);
  }
  if (typeof value === "number") {
    return new data.NumberData(value);
  }
  if (typeof value === "boolean") {
    return new data.BooleanData(value);
  }
  return new data.Null();
}
