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
 * into an ordinary string field of a generated response type, which is exactly
 * the shape no probe guessing at encodings would find.
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
 * Not an encoding of them: the bytes themselves, put where the content type
 * belongs. A client that did this would be transporting the image in an answer
 * that declares nothing of the kind.
 */
function theImagesBytesInPlaceOfAField(answered: ImageAnswer): ImageAnswer {
  const variant = structuredClone(answered);
  variant.record.content_type = bytesOf(answered).toString("binary");
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

  const withBytes = theImagesBytesInPlaceOfAField(answered);
  expect(withBytes.record.content_type).not.toBe(answered.record.content_type);
  expect(matches(withBytes, sent)).toBe(false);

  expect(proxy.calls()).toBe(1);
}, 600_000);
