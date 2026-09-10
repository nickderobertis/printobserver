/**
 * What makes the all-operation walk's own equality worth having.
 *
 * `live.test.ts` asserts that what each method answered carries, field for
 * field, what the **real** supervisor sent. An assertion nothing can fail is not
 * one, so two clients that a walk over emitted values would never catch are
 * driven through that same comparison here and are asserted to be refused.
 *
 * Both are about the one thing this system will not do: neither puts image bytes
 * in an answer legitimately — they put them where a client with a defect would,
 * at a field of a generated response type that declares nothing of the kind,
 * which is exactly the shape no probe guessing at encodings would find. One
 * carries a base64 of the image in an existing string field; the other carries
 * the bytes themselves, as a byte sequence rather than as any rendering of them
 * into text.
 *
 * Each variant is the published client with one thing done to what it answers,
 * which is what a defect is. The comparison is the committed one — `matches`,
 * the function the generated walk calls — so what is proven is that walk's own
 * assertion rather than a copy of it.
 */

import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, expect, test } from "bun:test";
import { Client } from "../src/client.ts";
import type { ImageAnswer } from "../src/contract.ts";
import { Recording, matches, same } from "./live.ts";
import { Standing, type Supervisor } from "./world.ts";

let standing: Standing;
let world: Supervisor;

beforeAll(async () => {
  standing = await Standing.standing(mkdtempSync(join(tmpdir(), "printobserver-falsifying-")));
  world = standing.at;
});

afterAll(async () => {
  await standing.stop();
});

/** The stored image's own bytes, opened at the path the supervisor answered. */
function bytesOf(answered: ImageAnswer): Buffer {
  const path = answered.path;
  if (typeof path !== "string") {
    throw new Error("the supervisor answered no path on its own host");
  }
  return readFileSync(path);
}

/**
 * A client that writes a base64 of the image into an existing string field.
 *
 * The field is the record's own digest, which is a string the contracts declare
 * and a caller reads — so nothing about the answer's shape has changed, and only
 * what it carries has.
 */
function base64OfTheImageIntoAStringField(answered: ImageAnswer): ImageAnswer {
  const variant = structuredClone(answered);
  variant.record.sha256 = bytesOf(answered).toString("base64");
  return variant;
}

/**
 * A client that returns the image's own bytes in place of a field's value.
 *
 * The bytes themselves, and no rendering of them into text: what this puts
 * where the content type belongs is exactly what a client whose own answer
 * carried that field as bytes would hand its caller. Decoding them into a
 * string first — as `binary`, as UTF-8, or otherwise — would be a string
 * substitution wearing an image's name, and would leave the one shape this is
 * about untested.
 *
 * The published types are left alone. A defect is a client that answers
 * something other than what the contract says, so the variant is built as the
 * document a client answers with rather than by giving a generated type a
 * field it does not have.
 */
function theImagesBytesInPlaceOfAField(answered: ImageAnswer): Record<string, unknown> {
  const variant = JSON.parse(JSON.stringify(answered)) as Record<string, unknown>;
  (variant.record as Record<string, unknown>).content_type = new Uint8Array(bytesOf(answered));
  return variant;
}

test("the equality the walk asserts refuses a client that carries the image", async () => {
  await using proxy = new Recording(world.server);
  const client = new Client({ server: proxy.url, actor: "operator" });

  const answered = await client.image(world.image_id);
  const sent = proxy.last().answer;

  // The image is the one this world opened, so what the variants carry is this
  // print's own image rather than a file that happened to be there.
  expect(answered.record.print_id).toBe(world.print_id);
  expect(answered.record.event_id).toBe(world.event_id);

  // The published client passes it, which is what makes the two refusals below
  // about the variants rather than about the comparison.
  same("image", answered, sent);

  const withBase64 = base64OfTheImageIntoAStringField(answered);
  expect(withBase64.record.sha256).not.toBe(answered.record.sha256);
  expect(matches(withBase64, sent)).toBe(false);

  // Not text, and not a decoding of the bytes into text: the image this world
  // stores opens `ff d8`, which no string of any encoding this system speaks
  // could carry, and what the variant puts in the field is the byte sequence
  // itself.
  const bytes = bytesOf(answered);
  expect(() => new TextDecoder("utf-8", { fatal: true }).decode(bytes)).toThrow();
  const withBytes = theImagesBytesInPlaceOfAField(answered);
  const substituted = (withBytes.record as Record<string, unknown>).content_type;
  expect(substituted).toBeInstanceOf(Uint8Array);
  expect((substituted as Uint8Array).length).toBe(bytes.length);
  expect(matches(withBytes, sent)).toBe(false);

  expect(proxy.calls()).toBe(1);
}, 600_000);
