/**
 * The client every generated method is a method of.
 *
 * It carries three things and no more: where the supervisor is, what
 * authenticates to it, and who this client acts as. The third is why no
 * generated method takes an actor: a call cannot act as somebody the client is
 * not.
 *
 * The one request this makes goes through the platform's own `fetch` rather
 * than a dependency. This client speaks to a supervisor over a loopback or a
 * local network — the supervisor is the thing beside the printer — and a
 * package a supervision tool takes as a dependency is better without a
 * transport stack of its own.
 */

import type { ActionAnswer, Actor, Range, RejectionReason } from "./contract.ts";
import { GeneratedClient } from "./contract.ts";
import { PrinterRefused, Refused, Rejected, Unreachable, Unreadable } from "./surface.ts";

/** The status a rejected action is answered under. */
const REJECTED_STATUS = 409;

/** The status an action the machine itself refused is answered under. */
const MACHINE_REFUSED_STATUS = 502;

/** The status a read or a write that was carried out is answered under. */
const SUCCESS_STATUS = 200;

/** The media type every operation takes a body in and answers in. */
const MEDIA_TYPE = "application/json";

/** How this client is pointed at one supervisor. */
export interface ClientOptions {
  /**
   * Where the supervisor answers, as its own configuration writes it —
   * `http://127.0.0.1:8420` — or as a bare `host:port`.
   */
  server: string;
  /** Who this client acts as. */
  actor: Actor;
  /**
   * What authenticates to it. This server requires none of its API callers; a
   * credential is for a deployment that has put something in front of it that
   * does.
   */
  credential?: string;
}

/**
 * A typed client of one printobserver supervisor.
 *
 * One method per public operation that supervisor serves, each generated from
 * the same checked-in description the server's own routes are folded out of.
 */
export class Client extends GeneratedClient {
  /** Where the supervisor answers, as an origin this client fetches from. */
  readonly origin: string;
  /** What authenticates to it, where anything does. */
  readonly credential: string | undefined;

  constructor(options: ClientOptions) {
    super(options.actor);
    this.origin = origin(options.server);
    this.credential = options.credential;
  }

  protected override async call<T>(
    method: string,
    target: string,
    asked: Array<[string, string]>,
    sending: Record<string, unknown> | undefined,
  ): Promise<T> {
    const headers: Record<string, string> = { Accept: MEDIA_TYPE };
    if (sending !== undefined) {
      headers["Content-Type"] = MEDIA_TYPE;
    }
    if (this.credential !== undefined) {
      headers.Authorization = `Bearer ${this.credential}`;
    }

    let answered: Response;
    try {
      answered = await fetch(`${this.origin}${written(target, asked)}`, {
        method,
        headers,
        ...(sending === undefined ? {} : { body: JSON.stringify(sending) }),
      });
    } catch (unreachable) {
      throw new Unreachable(this.origin, String(unreachable));
    }
    const said = await answered.text();

    if (answered.status === SUCCESS_STATUS) {
      return document<T>(answered.status, said);
    }
    if (answered.status === REJECTED_STATUS) {
      throw rejection(answered.status, said);
    }
    if (answered.status === MACHINE_REFUSED_STATUS) {
      throw machineRefusal(answered.status, said);
    }
    throw new Refused(answered.status, refusalText(said));
  }
}

/** One supervisor's address, as an origin `fetch` takes. */
function origin(server: string): string {
  const trimmed = server.trim().replace(/\/+$/, "");
  return trimmed.startsWith("http://") || trimmed.startsWith("https://")
    ? trimmed
    : `http://${trimmed}`;
}

/** The request target one call is made to, with what it asks for. */
function written(path: string, asked: Array<[string, string]>): string {
  if (asked.length === 0) {
    return path;
  }
  const query = asked
    .map(([name, value]) => `${encodeURIComponent(name)}=${encodeURIComponent(value)}`)
    .join("&");
  return `${path}?${query}`;
}

/** One answer, parsed. */
function document<T>(status: number, said: string): T {
  try {
    return JSON.parse(said) as T;
  } catch (unreadable) {
    throw new Unreadable(status, String(unreadable));
  }
}

/** What the supervisor said about a request it will not act on. */
function refusalText(said: string): string {
  try {
    const answer: unknown = JSON.parse(said);
    if (typeof answer === "object" && answer !== null && "error" in answer) {
      const stated = (answer as { error: unknown }).error;
      if (typeof stated === "string") {
        return stated;
      }
    }
  } catch {
    return "it said nothing this client can read";
  }
  return "it said nothing this client can read";
}

/** The policy's own refusal, as the answer to it carries it. */
function rejection(status: number, said: string): Error {
  const answer = document<ActionAnswer>(status, said);
  const decision = answer.record.decision;
  if (typeof decision !== "object" || !("rejected" in decision)) {
    return new Unreadable(
      status,
      "the supervisor refused this action and answered a record whose decision is not a refusal",
    );
  }
  const reason: RejectionReason = decision.rejected;
  let requested: number | undefined;
  let allowed: Range | undefined;
  if (typeof reason === "object" && "out_of_bounds" in reason) {
    requested = reason.out_of_bounds.requested;
    allowed = reason.out_of_bounds.allowed;
  }
  return new Rejected(reason, requested, allowed, answer);
}

/** The machine's own refusal of an action the policy accepted. */
function machineRefusal(status: number, said: string): Error {
  const answer = document<ActionAnswer>(status, said);
  return new PrinterRefused(
    answer.printer_refusal ?? "it said nothing this client can read",
    answer,
  );
}
