/**
 * What the client's contract check admits, held to declarations the readers
 * depend on: a member declared as the null scalar, which a valid document
 * sends as null, and a recovery allowance, whose `allowed` is false exactly
 * when its `refusal` is sent — and a member left optional, which a valid
 * document leaves out or sends of its declared shape, never as null. The walk
 * itself is held to a table it cannot read admitting nothing.
 */

import { expect, test } from "bun:test";
import { conforms, conformsIn } from "../src/shapes.ts";

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

test("an optional member is left out or of its shape, never null", () => {
  expect(conforms({ action: "recover_park" }, "RecoveryAction")).toBe(true);
  expect(conforms({ action: "recover_park", clearance_asserted: true }, "RecoveryAction")).toBe(
    true,
  );
  expect(conforms({ action: "recover_park", clearance_asserted: null }, "RecoveryAction")).toBe(
    false,
  );
});

test("a table the walk cannot read admits nothing", () => {
  const field = (shape: unknown, required: unknown = true) => ({
    struct: { fields: { member: { shape, required, paired_with: "" } }, variants: {} },
  });
  const unreadable: Record<string, unknown> = {
    "a shape that is no object": field("string"),
    "an unknown scalar": field({ scalar: "decimal" }),
    "a reference to nothing": field({ ref: "Missing" }),
    "a declaration of no kind": { member: {} },
    "a field with no requiredness": field({ scalar: "string" }, "yes"),
    "an arm that is no object": {
      struct: { fields: {}, variants: { lift: 1 }, tag_field: "action" },
    },
  };
  const documents: Record<string, unknown> = {
    "a shape that is no object": { member: "up" },
    "an unknown scalar": { member: "1.5" },
    "a reference to nothing": { member: "up" },
    "a declaration of no kind": { member: "up" },
    "a field with no requiredness": { member: "up" },
    "an arm that is no object": { action: "lift" },
  };
  for (const [name, document] of Object.entries(documents)) {
    expect([name, conformsIn(unreadable, document, name)]).toEqual([name, false]);
  }
  expect(conformsIn(unreadable, { member: "up" }, "undeclared")).toBe(false);
  const readable = { Named: field({ scalar: "string" }) };
  expect(conformsIn(readable, { member: "up" }, "Named")).toBe(true);
});
