/**
 * Every way a call can end, and what each one says.
 *
 * The generated walk drives the two a caller acts on — the policy's refusal and
 * the reason this client will not send. These are the rest: what each failure
 * says, so that a caller reading one knows what to do next.
 */

import { expect, test } from "bun:test";
import type { ActionAnswer, Range, RejectionReason } from "../src/contract.ts";
import {
  ClientError,
  GeneratedSurface,
  NoReason,
  PrinterRefused,
  Refused,
  Rejected,
  Unreachable,
  Unreadable,
  reasonGiven,
} from "../src/surface.ts";

/** An answer carrying nothing but the record a refusal is taken on. */
function answer(): ActionAnswer {
  return JSON.parse(
    '{"record":{"id":"a","print_id":"b","request":{"action":{"action":"pause",' +
      '"actor":"operator","reason":"because"},"actor":"operator","requested_at":"now"},' +
      '"decision":"accepted"}}',
  ) as ActionAnswer;
}

test("a reason that is nothing but whitespace is no reason", () => {
  expect(() => reasonGiven("   ")).toThrow(NoReason);
  expect(() => reasonGiven("")).toThrow(NoReason);
  expect(reasonGiven("because the print is failing")).toBeUndefined();
});

test("every failure is one a caller can tell from the others", () => {
  const failures: ClientError[] = [
    new ClientError("a call ended some way this client has no name for"),
    new Unreachable("127.0.0.1:8420", "connection refused"),
    new NoReason(),
    new Unreadable(200, "not JSON"),
    new Refused(404, "no such print"),
    new PrinterRefused("the printer is offline", answer()),
  ];

  for (const failure of failures) {
    expect(failure).toBeInstanceOf(ClientError);
    expect(failure.message.length).toBeGreaterThan(0);
  }
  expect(failures[1]?.message).toContain("127.0.0.1:8420");
  expect(failures[3]?.message).toContain("200");
  expect(failures[4]?.message).toContain("no such print");
  expect(failures[5]?.message).toContain("the printer is offline");
});

test("a refusal on a value says the value and the range", () => {
  const reason = JSON.parse(
    '{"out_of_bounds":{"adjustable":"feedrate","requested":9.9,"allowed":{"min":0.5,"max":1.5}}}',
  ) as RejectionReason;
  const allowed: Range = { min: 0.5, max: 1.5 };

  const refused = new Rejected(reason, 9.9, allowed, answer());

  expect(refused.message).toContain("9.9");
  expect(refused.message).toContain("0.5 to 1.5");
  expect(refused.reason).toEqual(reason);
  expect(refused.requested).toBe(9.9);
  expect(refused.allowed).toEqual(allowed);
});

test("a refusal on something other than a value says only the refusal", () => {
  const reason = "no_active_print" as RejectionReason;

  const refused = new Rejected(reason, undefined, undefined, answer());

  expect(refused.message).toContain("no_active_print");
  expect(refused.message).not.toContain("what is allowed");
});

test("a surface on its own makes no request", () => {
  const surface = new GeneratedSurface("operator");

  expect(surface.actor).toBe("operator");
  expect(() => reaching(surface)).toThrow("a surface makes no request");
});

/**
 * Reach the surface's own `call`, which every generated method is written
 * against and which only a subclass can name.
 */
function reaching(surface: GeneratedSurface): Promise<unknown> {
  class Reaching extends GeneratedSurface {
    ask(): Promise<unknown> {
      return this.call("GET", "/v1/anything", [], undefined);
    }
  }
  return Object.assign(new Reaching(surface.actor)).ask();
}
