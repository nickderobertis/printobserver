/**
 * What the client's contract check admits, held to declarations the readers
 * depend on: a member declared as the null scalar, which a valid document
 * sends as null, and a recovery allowance, whose `allowed` is false exactly
 * when its `refusal` is sent.
 */

import { expect, test } from "bun:test";
import { conforms } from "../src/shapes.ts";

/** An instant as the contracts write one. */
const AT = "2026-10-27T12:00:00Z";

test("a member declared null is read only as null", () => {
  expect(conforms({ status: "unchecked", since: null }, "DetectorHealth")).toBe(true);
  expect(conforms({ status: "unchecked", since: AT }, "DetectorHealth")).toBe(false);
  expect(conforms({ status: "available", since: AT }, "DetectorHealth")).toBe(true);
  expect(conforms({ status: "lost", since: AT }, "DetectorHealth")).toBe(false);
});

test("an allowance is read only when its two halves agree", () => {
  for (const agreeing of [
    { action: "recover_lift", allowed: true },
    { action: "recover_heat", allowed: false, refusal: "z_untrusted" },
  ]) {
    expect(conforms(agreeing, "ActionAllowance")).toBe(true);
  }
  for (const contradiction of [
    { action: "recover_lift", allowed: true, refusal: "z_untrusted" },
    { action: "recover_heat", allowed: false },
    { action: "recover_lift", allowed: true, refusal: null },
  ]) {
    expect(conforms(contradiction, "ActionAllowance")).toBe(false);
  }
});
