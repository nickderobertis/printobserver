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
import { PrinterRefused, Refused, Unreachable, Unreadable } from "../src/surface.ts";
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
