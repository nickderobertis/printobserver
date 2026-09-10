/**
 * Prove the installed Node client against a real running supervisor.
 *
 * This runs from the **installed package** rather than from this repository's
 * sources: what it imports is whatever `@printobserver/sdk` the environment
 * resolves. A smoke check that reached no server would say nothing about the
 * artifact, so it makes two real calls — a status read, and an image
 * materialization whose answered path it opens and whose bytes it checks
 * against the digest the image record itself declares.
 *
 *   node smoke.mjs --server http://127.0.0.1:8420 --print-id <id> --image-id <id>
 */

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { isAbsolute } from "node:path";
import { CONTRACT_VERSION, Client } from "@printobserver/sdk";

/** One named argument, or a stop saying which one is missing. */
function argument(name) {
  const at = process.argv.indexOf(`--${name}`);
  const value = at < 0 ? undefined : process.argv[at + 1];
  if (value === undefined) {
    process.stderr.write(`smoke: --${name} takes a value and was given none\n`);
    process.exit(2);
  }
  return value;
}

const server = argument("server");
const printId = argument("print-id");
const imageId = argument("image-id");

const client = new Client({ server, actor: "operator" });

const status = await client.status(printId);
if (status.print.id !== printId) {
  process.stderr.write(`the status read answered another print: ${status.print.id}\n`);
  process.exit(1);
}

const answered = await client.image(imageId);
const path = answered.path;
if (typeof path !== "string") {
  process.stderr.write("the image read answered no path on the server's own host\n");
  process.exit(1);
}
if (!isAbsolute(path)) {
  process.stderr.write(`the image read answered ${path}, which is not an absolute path\n`);
  process.exit(1);
}
const digest = createHash("sha256").update(readFileSync(path)).digest("hex");
const declared = answered.record.sha256;
if (digest !== declared) {
  process.stderr.write(`${path} is not the image the record declares (${digest} vs ${declared})\n`);
  process.exit(1);
}

process.stdout.write(
  `@printobserver/sdk smoke: contract ${CONTRACT_VERSION}, print ${status.print.state}, ` +
    `image ${declared.slice(0, 12)} at ${path}\n`,
);
