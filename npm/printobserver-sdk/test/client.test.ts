/**
 * What the client does with an answer that is not the one it asked for.
 *
 * The generated walk drives every method's success path and its refusal. These
 * are the paths beside them: an address written the several ways a supervisor's
 * own configuration writes it, and each answer this client will not act on.
 */

import { expect, test } from "bun:test";
import { Client } from "../src/client.ts";
import type { Actor } from "../src/contract.ts";
import { PrinterRefused, Refused, Rejected, Unreachable, Unreadable } from "../src/surface.ts";
import { Host } from "./host.ts";

const ACTOR: Actor = "operator";
const PRINT = "0198f0a1-2b3c-7d4e-8f90-123456789abc";

test("an address is taken as the supervisor's own configuration writes it", () => {
  for (const given of ["http://127.0.0.1:8420", "127.0.0.1:8420", "http://127.0.0.1:8420/"]) {
    expect(new Client({ server: given, actor: ACTOR }).origin).toBe("http://127.0.0.1:8420");
  }
});

test("a client acts as the actor it was made with, and carries its credential", () => {
  const client = new Client({ server: "127.0.0.1:8420", actor: ACTOR, credential: "a-token" });

  expect(client.actor).toBe(ACTOR);
  expect(client.credential).toBe("a-token");
});

test("nothing answering at the configured address is its own failure", async () => {
  // Port one is privileged and never listened on here, so this reaches nothing
  // without depending on which ports this host happens to have free.
  const client = new Client({ server: "http://127.0.0.1:1", actor: ACTOR });

  await expect(client.status(PRINT)).rejects.toBeInstanceOf(Unreachable);
});

test("an answer this client cannot read is said to be one", async () => {
  await using host = Host.answering(200, undefined);
  const client = new Client({ server: host.address, actor: ACTOR });

  await expect(client.status(PRINT)).rejects.toBeInstanceOf(Unreadable);
});

test("a request the supervisor will not act on carries its own words", async () => {
  await using host = Host.answering(404, { error: "no such print" });
  const client = new Client({ server: host.address, actor: ACTOR });

  await expect(client.status(PRINT)).rejects.toThrow("no such print");
});

test("a refusal the supervisor did not put in words is said to be unreadable", async () => {
  await using host = Host.answering(404, ["not an error answer"]);
  const client = new Client({ server: host.address, actor: ACTOR });

  const refused = await client.status(PRINT).catch((raised: unknown) => raised);

  expect(refused).toBeInstanceOf(Refused);
  expect((refused as Refused).detail).toBe("it said nothing this client can read");
});

test("an action the machine itself refused carries what the printer said", async () => {
  const answer = JSON.parse(
    '{"printer_refusal":"the printer is offline","record":{"id":"a","print_id":"b",' +
      '"request":{"action":{"action":"pause","actor":"operator","reason":"because"},' +
      '"actor":"operator","requested_at":"now"},"decision":"accepted"}}',
  );
  await using host = Host.answering(502, answer);
  const client = new Client({ server: host.address, actor: ACTOR });

  const refused = await client.pause(PRINT, "because").catch((raised: unknown) => raised);

  expect(refused).toBeInstanceOf(PrinterRefused);
  expect((refused as PrinterRefused).detail).toBe("the printer is offline");
});

test("a refusal whose record was not refused is said to be unreadable", async () => {
  const answer = JSON.parse(
    '{"record":{"id":"a","print_id":"b","request":{"action":{"action":"pause",' +
      '"actor":"operator","reason":"because"},"actor":"operator","requested_at":"now"},' +
      '"decision":"accepted"}}',
  );
  await using host = Host.answering(409, answer);
  const client = new Client({ server: host.address, actor: ACTOR });

  await expect(client.pause(PRINT, "because")).rejects.toBeInstanceOf(Unreadable);
});

test("what is asked for after the question mark is escaped where it must be", async () => {
  await using host = Host.answering(200, { events: [] });
  const client = new Client({ server: host.address, actor: ACTOR });

  await client.history(PRINT, 20);

  expect(host.received().target).toBe(`/v1/prints/${PRINT}/history?limit=20`);
});

/** A recovery's answer, carrying whatever decision a test gives it. */
function recovery(decision: unknown): Record<string, unknown> {
  return {
    action: { action: "recover_fan", percent: 50 },
    decision,
    detail: "recover_fan reached the printer, which refused it: the fan is stuck",
    events: [],
  };
}

test("a recovery the machine refused carries the supervisor's words", async () => {
  await using host = Host.answering(
    502,
    recovery({ executed: { failed: { reason: "the fan is stuck" } } }),
  );
  const client = new Client({ server: host.address, actor: ACTOR });

  const refused = await client.recoverFan(50, "because").catch((raised: unknown) => raised);

  if (!(refused instanceof Refused)) {
    throw new Error(`the refusal was not a Refused: ${String(refused)}`);
  }
  expect(refused.status).toBe(502);
  expect(refused.detail).toContain("the fan is stuck");
});

test("a refusal whose answer is not an object is unreadable", async () => {
  for (const status of [409, 502]) {
    for (const answer of [null, "refused", 7, [recovery({ rejected: "print_running" })]]) {
      await using host = Host.answering(status, answer);
      const client = new Client({ server: host.address, actor: ACTOR });

      await expect(client.recoverFan(50, "because")).rejects.toBeInstanceOf(Unreadable);
    }
  }
});

test("a recovery refused with bounds no reader can read is unreadable", async () => {
  for (const refused of [
    { out_of_bounds: { field: "percent", requested: "lots", allowed: {} } },
    { out_of_bounds: "everything" },
    { out_of_bounds: { requested: 150, allowed: { min: 0, max: 100 } } },
    null,
    { two: {}, arms: {} },
    { print_running: {} },
    "print_running",
    { z_untrusted: {} },
    { printer_cannot: { action: 7, detail: null } },
    { insufficient_clearance: { margin_mm: "5", nozzle_z_mm: 1, tallest_mm: 2 } },
    { print_running: { state: 7 } },
  ]) {
    await using host = Host.answering(409, recovery({ rejected: refused }));
    const client = new Client({ server: host.address, actor: ACTOR });

    await expect(client.recoverFan(50, "because")).rejects.toBeInstanceOf(Unreadable);
  }
});

test("a known refusal well formed arrives as a refusal", async () => {
  const refused = { print_running: { state: { unknown: "Connecting" } } };
  await using host = Host.answering(409, recovery({ rejected: refused }));
  const client = new Client({ server: host.address, actor: ACTOR });

  const raised = await client.recoverFan(50, "because").catch((error: unknown) => error);

  if (!(raised instanceof Rejected)) {
    throw new Error(`the refusal was not a Rejected: ${String(raised)}`);
  }
  expect(raised.reason).toEqual(refused);
});

/**
 * Recovery answers the contracts forbid, each refused under `409`: a recovery
 * action missing its parameter, naming no declared action, or carrying a
 * parameter of the wrong type; a refusal no vocabulary declares, or carrying a
 * printer state no vocabulary names; and an event entry of no declared shape.
 */
const MALFORMED_RECOVERIES = [
  { ...recovery({ rejected: "z_untrusted" }), action: { action: "recover_lift" } },
  { ...recovery({ rejected: "z_untrusted" }), action: { action: "recover_dance" } },
  { ...recovery({ rejected: "z_untrusted" }), action: { action: "recover_fan", percent: "lots" } },
  recovery({ rejected: "a_newer_reason" }),
  recovery({ rejected: { a_newer_reason: { anything: 1 } } }),
  recovery({ rejected: { print_running: { state: "melting" } } }),
  { ...recovery({ rejected: "z_untrusted" }), events: [{ kind: 7 }] },
];

test("a recovery refusal no contract declares is unreadable", async () => {
  for (const answer of MALFORMED_RECOVERIES) {
    await using host = Host.answering(409, answer);
    const client = new Client({ server: host.address, actor: ACTOR });

    await expect(client.recoverFan(50, "because")).rejects.toBeInstanceOf(Unreadable);
  }
});

/** A spool command's refusal, around one spool refusal cause. */
function spoolRefusal(cause: unknown): Record<string, unknown> {
  return {
    decision: { rejected: { spool_refused: { cause, message: "refused" } } },
    events: [],
  };
}

test("a spool refusal is read only with a cause the contracts declare", async () => {
  const spool = "0198f0a1-2b3c-7d4e-8f90-123456789abc";
  {
    await using host = Host.answering(
      409,
      spoolRefusal({ cause: "unknown_spool", spool_id: spool }),
    );
    const client = new Client({ server: host.address, actor: ACTOR });

    await expect(client.retireSpool(spool, "because")).rejects.toBeInstanceOf(Rejected);
  }
  for (const cause of [{ cause: "melted" }, { cause: "unknown_spool" }]) {
    await using host = Host.answering(409, spoolRefusal(cause));
    const client = new Client({ server: host.address, actor: ACTOR });

    await expect(client.retireSpool(spool, "because")).rejects.toBeInstanceOf(Unreadable);
  }
});

test("a machine refusal whose record is not an object and says nothing is unreadable", async () => {
  await using host = Host.answering(502, { record: null });
  const client = new Client({ server: host.address, actor: ACTOR });

  await expect(client.pause(PRINT, "because")).rejects.toBeInstanceOf(Unreadable);
});

test("a machine refusal of a recovery that did not fail is unreadable", async () => {
  for (const decision of [
    { rejected: "z_untrusted" },
    { executed: "succeeded" },
    null,
    { executed: { failed: { reason: "the fan is stuck" } }, rejected: "z_untrusted" },
    { executed: { failed: { reason: "the fan is stuck" }, succeeded: true } },
  ]) {
    await using host = Host.answering(502, recovery(decision));
    const client = new Client({ server: host.address, actor: ACTOR });

    await expect(client.recoverFan(50, "because")).rejects.toBeInstanceOf(Unreadable);
  }
});

test("a policy refusal whose decision has two arms is unreadable", async () => {
  await using host = Host.answering(
    409,
    recovery({ rejected: "z_untrusted", executed: "succeeded" }),
  );
  const client = new Client({ server: host.address, actor: ACTOR });

  await expect(client.recoverFan(50, "because")).rejects.toBeInstanceOf(Unreadable);
});

test("a refusal whose bounds overflow every float is unreadable", async () => {
  for (const bound of ['"requested": 1e400', '"allowed": {"min": 0, "max": 1e400}']) {
    const bounds = bound.startsWith('"requested"')
      ? `{"field": "percent", ${bound}, "allowed": {"min": 0, "max": 100}}`
      : `{"field": "percent", "requested": 150, ${bound}}`;
    await using host = Host.answeringText(
      409,
      `{"action": {"action": "recover_fan", "percent": 50}, "decision": {"rejected": {"out_of_bounds": ${bounds}}}, "detail": "refused", "events": []}`,
    );
    const client = new Client({ server: host.address, actor: ACTOR });

    await expect(client.recoverFan(50, "because")).rejects.toBeInstanceOf(Unreadable);
  }
});

test("a recovery refusal missing its parts is unreadable", async () => {
  const whole = recovery({ rejected: "z_untrusted" });
  const { events: _events, ...eventless } = whole;
  for (const answer of [{ ...whole, action: {} }, eventless]) {
    await using host = Host.answering(409, answer);
    const client = new Client({ server: host.address, actor: ACTOR });

    await expect(client.recoverFan(50, "because")).rejects.toBeInstanceOf(Unreadable);
  }
});
