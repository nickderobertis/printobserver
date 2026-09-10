#!/usr/bin/env node
/**
 * Run the `printobserver` program this install resolved for the caller's platform.
 *
 * This launcher carries no program of its own. It declares one package per
 * supported platform as an optional dependency, and the caller's package
 * manager installs exactly the one their operating system and processor
 * select; this finds that package and runs the program inside it.
 *
 * Nothing here compiles anything. The host this is installed on is the small
 * machine beside the printer, and the whole reason the program is shipped
 * already built is that that machine is the worst place to build it.
 */

import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";

/** What each supported platform's package is called, by what Node reports. */
const PACKAGES = {
  "linux-x64": "@printobserver/cli-linux-x64",
  "linux-arm64": "@printobserver/cli-linux-arm64",
};

const platform = `${process.platform}-${process.arch}`;
const name = PACKAGES[platform];
if (name === undefined) {
  process.stderr.write(
    `printobserver: this package ships no program for ${platform}. The platforms it ` +
      `ships for are ${Object.keys(PACKAGES).join(", ")}. Install printobserver with ` +
      `the bundled install script instead, or build it from source.\n`,
  );
  process.exit(1);
}

const require = createRequire(import.meta.url);
let program;
try {
  program = require.resolve(`${name}/bin/printobserver`);
} catch {
  process.stderr.write(
    `printobserver: the program for ${platform} is not installed. Its package is ` +
      `${name}, which this package declares as an optional dependency; an install ` +
      `run with optional dependencies disabled leaves it out. Reinstall with them ` +
      `enabled.\n`,
  );
  process.exit(1);
}

const ran = spawnSync(program, process.argv.slice(2), { stdio: "inherit" });
if (ran.error !== undefined) {
  process.stderr.write(`printobserver: ${program} could not be run: ${ran.error.message}\n`);
  process.exit(1);
}
process.exit(ran.status === null ? 1 : ran.status);
