/**
 * The one journey all three clients drive, in TypeScript.
 *
 * Nine steps against a **real supervisor backed by a real OctoPrint** — the
 * instance `just octoprint-up` started, with its virtual printer. Nothing here
 * stands in for a layer: the client is the published one, the server is the
 * program this repository builds, and the machine is a real one.
 *
 * The same nine steps run in the Rust and the Python clients, in the same
 * order, asserting the same normalized answers. Three clients running three
 * different journeys would prove three different products.
 *
 * Four orderings are load-bearing. The manifest is written before the print is
 * started, so the print runs under it. The accepted adjustment and the rejected
 * one both happen after the print has started and before history is read, so
 * history has them to account for. The print is cancelled last.
 */

import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { isAbsolute } from "node:path";
import { afterAll, beforeAll, expect, test } from "bun:test";
import { Client } from "../src/client.ts";
import type { JobManifest, PrinterState } from "../src/contract.ts";
import { NoReason, Rejected } from "../src/surface.ts";
import { Standing, type Supervisor } from "./world.ts";

/** The reason every mutating step of this walk carries. */
const REASON = "a printer-integration journey is asking";

/** How long a bounded adjustment stands for, in whole seconds. */
const DURATION = 60;

/** The feedrate factors the accepted and the refused adjustments ask for. */
const INSIDE = 1.1;
const OUTSIDE = 9.9;

/** How long the machine is given to reach a state a step needs. */
const PATIENCE_MS = 180_000;

let standing: Standing;
let world: Supervisor;

beforeAll(async () => {
  standing = await Standing.standing(mkdtempSync(join(tmpdir(), "printobserver-node-world-")));
  world = standing.at;
});

afterAll(async () => {
  await standing.stop();
});

/** The manifest this walk writes, which the print then runs under. */
function manifest(fileName: string): JobManifest {
  return {
    file_name: fileName,
    material: "PLA",
    nozzle_diameter_mm: 0.4,
    slicer_profile: "a journey's own profile",
    allowed: { feedrate: { min: 0.9, max: 1.2 } },
    metadata: {},
  };
}

/** Wait until the machine reports one of these states. */
async function until(client: Client, printId: string, wanted: PrinterState[]): Promise<void> {
  const deadline = Date.now() + PATIENCE_MS;
  let last: unknown = "nothing was reported";
  while (Date.now() < deadline) {
    const printer = (await client.status(printId)).printer ?? null;
    if (printer !== null) {
      last = printer.connection;
      if (wanted.includes(printer.connection)) {
        return;
      }
    }
    await Bun.sleep(500);
  }
  throw new Error(
    `the machine reported ${JSON.stringify(last)} and this step needs one of ${wanted}`,
  );
}

test("the same nine steps are answered against a real OctoPrint", async () => {
  const client = new Client({ server: world.server, actor: "operator" });

  // 1. Read status.
  expect((await client.status(world.print_id)).print.id).toBe(world.print_id);

  // 2. Read context, and materialize its latest image.
  expect((await client.context(world.print_id)).context.print.id).toBe(world.print_id);
  const image = await client.image(world.image_id);
  expect(typeof image.path).toBe("string");
  expect(isAbsolute(image.path as string)).toBe(true);
  expect(
    createHash("sha256")
      .update(readFileSync(image.path as string))
      .digest("hex"),
  ).toBe(image.record.sha256);

  // 3. Write a job manifest and read it back, before the print is started.
  const wanted = manifest(world.file_name);
  const written = await client.manifestSet(world.print_id, REASON, wanted);
  expect(written.manifest).toEqual(wanted);
  expect((await client.manifestGet(world.print_id)).manifest).toEqual(written.manifest);

  // Setting the running print down is this walk's own setup rather than one of
  // the nine steps; it starts one again at the end.
  await client
    .cancel(world.print_id, "making room for the step that starts one")
    .catch(() => undefined);
  await until(client, world.print_id, ["operational"]);

  // 4. Start a print.
  const started = await client.startPrint(world.print_id, world.file_name, wanted, REASON);
  expect(started.record.decision).toBe("accepted");
  await until(client, world.print_id, ["printing"]);

  // 5. One accepted adjustment, carrying a reason and a duration.
  const adjusted = await client.setFeedrateFactor(world.print_id, INSIDE, REASON, DURATION);
  expect(adjusted.record.decision).toBe("accepted");
  expect(adjusted.intervention?.applied_value).toBe(INSIDE);

  // 6. One adjustment outside the effective bounds, and the typed rejection.
  const refused = await client
    .setFeedrateFactor(world.print_id, OUTSIDE, REASON)
    .catch((raised: unknown) => raised);
  expect(refused).toBeInstanceOf(Rejected);
  const rejection = refused as Rejected;
  expect(rejection.reason).toHaveProperty("out_of_bounds");
  expect(rejection.requested).toBe(OUTSIDE);
  expect(rejection.allowed?.max).toBeLessThan(OUTSIDE);

  // 7. One mutating call whose reason is empty, refused here with no request
  //    reaching the server.
  const before = (await client.history(world.print_id)).events.length;
  for (const empty of ["", "   "]) {
    const unreasoned = await client.pause(world.print_id, empty).catch((r: unknown) => r);
    expect(unreasoned).toBeInstanceOf(NoReason);
  }
  // Pointed at an address nothing is listening on, the same call still refuses
  // for want of a reason rather than for want of a server — which is what "no
  // request reached the server" means.
  const nowhere = new Client({ server: "http://127.0.0.1:1", actor: "operator" });
  expect(await nowhere.pause(world.print_id, "").catch((r: unknown) => r)).toBeInstanceOf(NoReason);
  expect((await client.history(world.print_id)).events.length).toBe(before);

  // 8. Read history, and find the accepted action, its decision and outcome.
  const events = (await client.history(world.print_id, 200)).events;
  const asked = events.find(
    (event) =>
      event.kind === "action_requested" &&
      event.payload.action.action === "set_feedrate_factor" &&
      event.payload.action.factor === INSIDE,
  );
  expect(asked).toBeDefined();
  const actionId =
    asked?.kind === "action_requested" ? asked.payload.action_id : "no action was found";
  const rejected = events.flatMap((event) =>
    event.kind === "action_rejected" ? [event.payload.action_id] : [],
  );
  const executed = events.flatMap((event) =>
    event.kind === "action_executed" ? [event.payload.action_id] : [],
  );
  expect(rejected).not.toContain(actionId);
  expect(executed).toContain(actionId);
  expect(rejected.length).toBeGreaterThan(0);

  // 9. Cancel the print, last.
  expect((await client.cancel(world.print_id, REASON)).record.decision).toBe("accepted");

  // Teardown rather than a tenth step.
  await until(client, world.print_id, ["operational"]);
  await client.startPrint(
    world.print_id,
    world.file_name,
    wanted,
    "putting the hold print back where the bring-up left it",
  );
}, 600_000);
