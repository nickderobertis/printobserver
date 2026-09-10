/**
 * The reason is not something a caller of this client can leave out.
 *
 * Every mutating method takes it as an argument, so a call that omits it does
 * not compile — which means there is no run-time refusal for an omission for a
 * journey to inspect, and no journey could tell the difference between a client
 * that requires it and one that quietly defaults it. This is where that
 * difference is checked: the real type checker runs over a fixture that omits
 * it, and is asserted to refuse.
 */

import { expect, test } from "bun:test";

test("a mutating call that leaves the reason out does not compile", async () => {
  const checked = Bun.spawn(
    [
      // The type checker the locked install put in the tree, by its own path:
      // `bunx` would resolve it again, and this suite runs beside every other
      // project's.
      `${import.meta.dir}/../../../node_modules/.bin/tsc`,
      "--noEmit",
      "--strict",
      "--target",
      "ES2023",
      "--module",
      "ESNext",
      "--moduleResolution",
      "bundler",
      "--allowImportingTsExtensions",
      "--lib",
      "ES2023,DOM",
      "test/fixtures/omitted-reason.fixture.ts",
    ],
    { cwd: `${import.meta.dir}/..`, stdout: "pipe", stderr: "pipe" },
  );
  const said = await new Response(checked.stdout).text();
  const status = await checked.exited;

  expect(status).not.toBe(0);
  expect(said).toContain("omitted-reason.fixture.ts");
  expect(said).toContain("Expected 2 arguments, but got 1");
  // Generous, and for one reason: this runs a whole type check, beside every
  // other project's own tier. A default timeout would fail on a loaded machine
  // and say nothing about the client.
}, 300_000);
