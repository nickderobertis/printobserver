// The verdict a required macOS context reports from a Linux runner.
//
// `ci.yml` runs the gate and the printer integration tier for macOS in ONE
// job, `macos`, so one pull request occupies one shared macOS slot rather than
// two. Branch protection still requires `gate (macos-aarch64)` and
// `integration (macos-aarch64)` by name, so two small Linux jobs carry those
// names. Each reads what the forge says happened — `toJSON(needs)` — and
// reports exactly that:
//
//   * the macOS lane ran and its own step for this lane succeeded: success;
//   * the classifier said this run owes no macOS lane (a non-pull-request run,
//     or release automation's version/changelog-only pull request) and the lane
//     was skipped: success, saying in so many words that nothing ran on macOS;
//   * anything else — the classifier failed, answered nothing readable, the
//     lane was owed and did not run, was cancelled, or this lane's step did not
//     succeed — is a failure naming which.
//
// A carrier never reads a skipped or cancelled macOS lane as a pass, and never
// says macOS tests ran when they did not.

import { isObject } from "./util.mjs";

/** The lanes the one macOS job carries, by the step outcome it publishes for each. */
export const LANES = ["gate", "integration"];

/** The job keys the carriers need, as `ci.yml` names them. */
export const CLASSIFY_JOB = "classify";
export const MACOS_JOB = "macos";
export const REQUIRED_OUTPUT = "macos-required";
export const REASON_OUTPUT = "reason";

/**
 * @typedef {{ok: boolean, message: string}} Verdict
 */

/**
 * The verdict one carrier reports.
 *
 * @param {string} lane
 * @param {unknown} needs what the forge hands a job as `toJSON(needs)`
 * @returns {Verdict}
 */
export function carry(lane, needs) {
  if (!LANES.includes(lane)) {
    return fail(
      `\`${lane}\` is not a lane the macOS job carries (${LANES.join(", ")}); fix the lane ci.yml's carrier passes to \`just macos-carry\``,
    );
  }
  const context = `${lane} (macos-aarch64)`;
  if (!isObject(needs)) {
    return fail(
      `${context}: the forge handed no job results to judge; check that the carrier's NEEDS is \`toJSON(needs)\` and re-run the workflow`,
    );
  }
  const classified = needs[CLASSIFY_JOB];
  const macos = needs[MACOS_JOB];
  if (!isObject(classified) || !isObject(macos)) {
    return fail(
      `${context}: the results of \`${CLASSIFY_JOB}\` and \`${MACOS_JOB}\` are not both present; the carrier must need both jobs`,
    );
  }
  if (classified.result !== "success") {
    return fail(
      `${context}: the classifier did not succeed (${String(classified.result)}), so whether the macOS lane was owed is unknown; read the \`${CLASSIFY_JOB}\` job's log, fix what it names and re-run the workflow`,
    );
  }
  const outputs = isObject(classified.outputs) ? classified.outputs : {};
  const required = outputs[REQUIRED_OUTPUT];
  const reason = typeof outputs[REASON_OUTPUT] === "string" ? outputs[REASON_OUTPUT] : "";
  if (required !== "true" && required !== "false") {
    return fail(
      `${context}: the classifier answered ${JSON.stringify(required)} for \`${REQUIRED_OUTPUT}\`, which is neither true nor false; read the \`${CLASSIFY_JOB}\` job's log and re-run the workflow`,
    );
  }
  if (required === "false" && macos.result === "skipped") {
    return pass(
      `${context}: no macOS ${lane} ran on this run, and none was owed — ${reason || "the classifier said so"}. This context records an authorized omission, not a macOS pass.`,
    );
  }
  if (macos.result !== "success" && macos.result !== "failure") {
    return fail(
      `${context}: the macOS lane was ${String(macos.result)}, so its ${lane} did not run to a verdict; re-run the \`macos-aarch64 (gate, integration)\` job`,
    );
  }
  const laneOutputs = isObject(macos.outputs) ? macos.outputs : {};
  const outcome = laneOutputs[lane];
  if (outcome === "success") {
    return pass(`${context}: the macOS lane ran its ${lane} on macos-aarch64 and it succeeded.`);
  }
  return fail(
    `${context}: the macOS lane's ${lane} ${outcome ? `ended ${outcome}` : "reported no outcome"} (the job ended ${String(macos.result)}); read the \`${lane}\` step of the \`macos-aarch64 (gate, integration)\` job, fix what it names and re-run that job`,
  );
}

/**
 * @param {string} message
 * @returns {Verdict}
 */
function pass(message) {
  return { ok: true, message };
}

/**
 * @param {string} message
 * @returns {Verdict}
 */
function fail(message) {
  return { ok: false, message };
}
