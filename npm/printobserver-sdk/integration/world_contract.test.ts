import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { expect, test } from "bun:test";

import { SETUP_TIMEOUT_MS } from "./world";

const REPO_ROOT = resolve(dirname(new URL(import.meta.url).pathname), "../../..");

test("the clients give their shared world one startup budget", () => {
  const python = readFileSync(
    resolve(REPO_ROOT, "python/printobserver-sdk/integration/supervisor_world.py"),
    "utf8",
  );
  const seconds = python.match(/^STARTUP_TIMEOUT_SECONDS = (\d+)$/m)?.[1];
  expect(seconds).toBeDefined();
  expect(SETUP_TIMEOUT_MS).toBe(Number(seconds) * 1000);
});
