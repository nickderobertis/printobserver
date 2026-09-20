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

/**
 * What each supported platform's package is called, by what Node reports.
 *
 * `just check-repo`'s `platform-facts` holds this map to AGENTS.md's
 * supported-platform list in both directions: a platform the install path
 * targets is here, and nothing else is.
 */
const PACKAGES = {
  "linux-x64": "@printobserver/cli-linux-x64",
  "linux-arm64": "@printobserver/cli-linux-arm64",
  "darwin-arm64": "@printobserver/cli-darwin-arm64",
  "win32-x64": "@printobserver/cli-win32-x64",
  "win32-arm64": "@printobserver/cli-win32-arm64",
};

/**
 * What the program's own file is called inside its package: Windows finds a
 * program by its suffix, and a file with none is one it will not run.
 */
const PROGRAM = process.platform === "win32" ? "printobserver.exe" : "printobserver";

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
  program = require.resolve(`${name}/bin/${PROGRAM}`);
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
