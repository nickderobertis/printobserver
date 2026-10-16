#!/usr/bin/env node
// The macOS runner policy's command line, as the workflows call it.
//
//   classify --event <JSON path> --base <git ref> --head <git ref>
//       Print `{isReleaseOnly, macosRequired, reason}` for the run whose event
//       payload is at <JSON path>. The event's name is the forge's own
//       `GITHUB_EVENT_NAME`, read off the shape of the payload where unset.
//   outputs <classification JSON path, or - for standard input>
//       Print that classification as the `name=value` lines a step appends to
//       `GITHUB_OUTPUT` — `macos-required` and `reason`, the two the
//       workflow reads — and the classification, one line, on standard error.
//   carry --lane gate|integration
//       Judge one required macOS context from `NEEDS`, the `toJSON(needs)` the
//       forge hands the carrier job; say why — on standard output for a pass,
//       on standard error with what to do next for a failure, and in the step
//       summary either way — and exit 0 for a pass and 1 for a failure.
//
// Exit status 2 is a call this does not take, before anything is judged.

import { appendFileSync, readFileSync } from "node:fs";
import { carry } from "./lib/carry.mjs";
import { isObject, message } from "./lib/util.mjs";

const USAGE =
  "usage: macos-policy.mjs classify --event <JSON path> --base <git ref> --head <git ref>\n" +
  "       macos-policy.mjs outputs <classification JSON path | ->\n" +
  "       macos-policy.mjs carry --lane gate|integration   (reads NEEDS)";

/**
 * @param {string[]} argv
 * @returns {Promise<number>}
 */
export async function main(argv) {
  const [command, ...rest] = argv;
  switch (command) {
    case "classify":
      return await classifyCommand(rest);
    case "outputs":
      return outputsCommand(rest);
    case "carry":
      return carryCommand(rest);
    default:
      return usage(command ? `\`${command}\` is not a command this takes` : "no command given");
  }
}

/**
 * @param {string[]} args
 * @returns {Promise<number>}
 */
async function classifyCommand(args) {
  const options = parseOptions(args, ["--event", "--base", "--head"]);
  if (typeof options === "string") {
    return usage(options);
  }
  // Imported here rather than at the top: the carriers run with no installed
  // dependencies, and only the classifier parses TOML.
  const { classify } = await import("./lib/classify.mjs");
  const classification = classify({
    eventPath: options["--event"],
    base: options["--base"],
    head: options["--head"],
    eventName: process.env.GITHUB_EVENT_NAME ?? "",
  });
  process.stdout.write(`${JSON.stringify(classification)}\n`);
  return 0;
}

/**
 * @param {string[]} args
 * @returns {number}
 */
function outputsCommand(args) {
  if (args.length !== 1) {
    return usage("outputs takes one classification file");
  }
  const source = args[0] === "-" ? "standard input" : args[0];
  let classification;
  try {
    classification = JSON.parse(readFileSync(args[0] === "-" ? 0 : args[0], "utf8"));
  } catch (unreadable) {
    process.stderr.write(
      `macos-policy: ${source} is not a classification: ${message(unreadable)}; hand \`outputs\` what \`classify\` printed\n`,
    );
    return 1;
  }
  if (
    !isObject(classification) ||
    typeof classification.macosRequired !== "boolean" ||
    typeof classification.isReleaseOnly !== "boolean" ||
    typeof classification.reason !== "string"
  ) {
    process.stderr.write(
      `macos-policy: ${source} is not shaped like a classification ({isReleaseOnly, macosRequired, reason}); hand \`outputs\` what \`classify\` printed\n`,
    );
    return 1;
  }
  process.stderr.write(`${JSON.stringify(classification)}\n`);
  const reason = classification.reason.replace(/[\r\n]+/g, " ");
  process.stdout.write(`macos-required=${classification.macosRequired}\nreason=${reason}\n`);
  return 0;
}

/**
 * @param {string[]} args
 * @returns {number}
 */
function carryCommand(args) {
  const options = parseOptions(args, ["--lane"]);
  if (typeof options === "string") {
    return usage(options);
  }
  let needs;
  try {
    needs = JSON.parse(process.env.NEEDS ?? "");
  } catch {
    needs = undefined;
  }
  const verdict = carry(options["--lane"], needs);
  (verdict.ok ? process.stdout : process.stderr).write(`${verdict.message}\n`);
  const summary = process.env.GITHUB_STEP_SUMMARY;
  if (summary) {
    appendFileSync(summary, `${verdict.ok ? "PASS" : "FAIL"}: ${verdict.message}\n`);
  }
  return verdict.ok ? 0 : 1;
}

/**
 * Every named option, each given exactly once with a value; a message where not.
 *
 * @param {string[]} args
 * @param {string[]} names
 * @returns {Record<string, string> | string}
 */
function parseOptions(args, names) {
  /** @type {Record<string, string>} */
  const found = {};
  for (let at = 0; at < args.length; at += 2) {
    const name = args[at];
    const value = args[at + 1];
    if (!names.includes(name)) {
      return `\`${name}\` is not an option this takes`;
    }
    if (value === undefined || value === "") {
      return `${name} takes a value`;
    }
    if (name in found) {
      return `${name} is given twice`;
    }
    found[name] = value;
  }
  const missing = names.filter((name) => !(name in found));
  return missing.length ? `missing ${missing.join(", ")}` : found;
}

/**
 * @param {string} why
 * @returns {number}
 */
function usage(why) {
  process.stderr.write(`macos-policy: ${why}\n${USAGE}\n`);
  return 2;
}

process.exitCode = await main(process.argv.slice(2));
