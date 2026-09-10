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
import { Recording, same } from "./live.ts";
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

/**
 * Step seven: a mutating call whose reason is empty reaches no server at all.
 *
 * Unchanged history proves nothing on its own — a request the server took and
 * recorded nowhere would leave it unchanged too — and neither does a client
 * pointed at an address nothing listens on, which reaches no server whatever it
 * is asked. So the calls are made through a recording proxy in front of the real
 * supervisor, and what is asserted is that not one of them went through it.
 */
async function unreasoned(world: Supervisor): Promise<void> {
  await using proxy = new Recording(world.server);
  const client = new Client({ server: proxy.url, actor: "operator" });

  // A read first: "nothing went through" is a claim about the calls below, and
  // against a proxy nothing could reach it would be true of anything at all.
  const before = await client.history(world.print_id);
  same("history", before, proxy.last().answer);
  const reads = proxy.calls();
  expect(reads).toBe(1);

  for (const empty of ["", "   "]) {
    const refused = await client.pause(world.print_id, empty).catch((r: unknown) => r);
    expect(refused).toBeInstanceOf(NoReason);
  }
  expect(proxy.calls()).toBe(reads);

  // Pointed at an address nothing is listening on, the same call still refuses
  // for want of a reason rather than for want of a server — which is the
  // client's own half of the same claim.
  const nowhere = new Client({ server: "http://127.0.0.1:1", actor: "operator" });
  expect(await nowhere.pause(world.print_id, "").catch((r: unknown) => r)).toBeInstanceOf(NoReason);

  const after = await client.history(world.print_id);
  expect(after.events.length).toBe(before.events.length);
  expect(proxy.calls()).toBe(reads + 1);
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

  // journey step 1: status
  expect((await client.status(world.print_id)).print.id).toBe(world.print_id);

  // journey step 2: context
  expect((await client.context(world.print_id)).context.print.id).toBe(world.print_id);
  const image = await client.image(world.image_id);
  expect(typeof image.path).toBe("string");
  expect(isAbsolute(image.path as string)).toBe(true);
  expect(
    createHash("sha256")
      .update(readFileSync(image.path as string))
      .digest("hex"),
  ).toBe(image.record.sha256);

  // journey step 3: manifest — written before the print is started, so the print
  //                            runs under it.
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

  // journey step 4: start
  const started = await client.startPrint(world.print_id, world.file_name, wanted, REASON);
  expect(started.record.decision).toBe("accepted");
  await until(client, world.print_id, ["printing"]);

  // journey step 5: adjustment — accepted, carrying a reason and a duration.
  const adjusted = await client.setFeedrateFactor(world.print_id, INSIDE, REASON, DURATION);
  expect(adjusted.record.decision).toBe("accepted");
  expect(adjusted.intervention?.applied_value).toBe(INSIDE);

  // journey step 6: refusal — outside the effective bounds, and the typed
  //                           rejection it is refused by.
  const refused = await client
    .setFeedrateFactor(world.print_id, OUTSIDE, REASON)
    .catch((raised: unknown) => raised);
  expect(refused).toBeInstanceOf(Rejected);
  const rejection = refused as Rejected;
  expect(rejection.reason).toHaveProperty("out_of_bounds");
  expect(rejection.requested).toBe(OUTSIDE);
  expect(rejection.allowed?.max).toBeLessThan(OUTSIDE);

  // journey step 7: unreasoned — one call whose reason is empty, refused here
  //                              with no request reaching the server.
  await unreasoned(world);

  // journey step 8: history — read after the adjustments, so it has them to
  //                           account for.
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

  // journey step 9: cancel
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
