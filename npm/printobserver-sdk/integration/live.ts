/**
 * What the walk against a real supervisor stands on.
 *
 * Two things, and the second is what makes the first worth having.
 *
 * `Proxy` sits between the client and the **real** supervisor and forwards
 * every request to it, recording what went and what came back. That is where
 * the body a comparison is against comes from: the answer this supervisor
 * actually sent, rather than a document a test wrote for it.
 *
 * `matches` is the comparison. What a method answered is held against that
 * captured body, field for field — so a client that put anything of its own
 * into an answer fails on the equality rather than needing a probe that guesses
 * what it put there. `falsifying.test.ts` drives two such clients through it.
 */

import { expect } from "bun:test";
import type { Client, PrinterState } from "../src/index.ts";

/** One request that went to the supervisor, and the answer it sent back. */
export interface Exchange {
  /** The method the call was made by. */
  method: string;
  /** The whole request target, question mark and all. */
  target: string;
  /** The body the call carried, empty where it carried none. */
  body: string;
  /** The status the supervisor answered under. */
  status: number;
  /** The whole document the supervisor answered with. */
  answer: string;
}

/** How long the machine is given to reach a state a step needs. */
export const PATIENCE_MS = 180_000;

/**
 * A recording proxy in front of one supervisor.
 *
 * `Recording` rather than `Proxy`: the platform has a `Proxy` of its own, and a
 * name that shadowed it would be one a reader had to keep two meanings for.
 */
export class Recording {
  private readonly seen: Exchange[] = [];
  private readonly server: ReturnType<typeof Bun.serve>;

  constructor(onward: string) {
    const seen = this.seen;
    const to = onward.replace(/\/+$/, "");
    this.server = Bun.serve({
      port: 0,
      hostname: "127.0.0.1",
      async fetch(request: Request): Promise<Response> {
        const asked = new URL(request.url);
        const target = `${asked.pathname}${asked.search}`;
        const body = await request.text();
        const answered = await fetch(`${to}${target}`, {
          method: request.method,
          headers: { Accept: "application/json", "Content-Type": "application/json" },
          ...(body === "" ? {} : { body }),
        });
        const said = await answered.text();
        seen.push({
          method: request.method,
          target,
          body,
          status: answered.status,
          answer: said,
        });
        return new Response(said, {
          status: answered.status,
          headers: { "Content-Type": "application/json" },
        });
      },
    });
  }

  /** Where this proxy answers, as a client's own configuration writes it. */
  get url(): string {
    return `http://127.0.0.1:${this.server.port}`;
  }

  /**
   * The last exchange that went through this proxy.
   *
   * @throws {Error} If nothing has, which is a call that was never made.
   */
  last(): Exchange {
    const seen = this.seen[this.seen.length - 1];
    if (seen === undefined) {
      throw new Error("no call reached the supervisor through this proxy");
    }
    return seen;
  }

  /** How many calls have gone through it. */
  calls(): number {
    return this.seen.length;
  }

  /** Stop forwarding, leaving no listener behind. */
  async [Symbol.asyncDispose](): Promise<void> {
    await this.server.stop(true);
  }
}

/** Whether what a method answered is, field for field, what was sent. */
export function matches(answered: unknown, captured: string): boolean {
  return Bun.deepEquals(answered, JSON.parse(captured), true);
}

/**
 * Assert that what a method answered is what the supervisor sent.
 *
 * @throws {Error} If it is not, showing both.
 */
export function same(operation: string, answered: unknown, captured: string): void {
  if (!matches(answered, captured)) {
    expect(answered, `${operation} answered something other than what was sent`).toEqual(
      JSON.parse(captured),
    );
  }
}

/**
 * Wait until the machine reports the state one step needs.
 *
 * A real printer is a state machine and the policy refuses an action that is
 * not valid from where it is, so a walk that went on regardless would assert
 * against a machine that was somewhere else.
 *
 * @throws {Error} If it does not, saying what it reported instead.
 */
export async function ready(client: Client, printId: string, wanted: string): Promise<void> {
  if (wanted === "") {
    return;
  }
  const deadline = Date.now() + PATIENCE_MS;
  let last: unknown = "nothing was reported";
  while (Date.now() < deadline) {
    const printer = (await client.status(printId)).printer ?? null;
    if (printer !== null) {
      last = printer.connection;
      if ((printer.connection as PrinterState) === wanted) {
        return;
      }
    }
    await Bun.sleep(500);
  }
  throw new Error(
    `the machine reported ${JSON.stringify(last)} and the next step needs \`${wanted}\``,
  );
}
