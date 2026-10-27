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

import type {
  ActionAnswer,
  Actor,
  Range,
  RecoveryAnswer,
  RecoveryRejection,
  RejectionReason,
  SpoolCommandAnswer,
} from "./contract.ts";
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
   * What authenticates to it. The supervisor serves no versioned operation to a
   * caller that does not present a credential it admits — the operator's, whose
   * verifier it is configured with and which `printobserver credential issue`
   * issues, or the one a supervision turn is handed — and a call it refuses for
   * that throws `Refused` with status 401. A call whose actor is not the
   * identity that credential is throws it with status 403.
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

/** Whether one parsed value is a JSON object, whose members can then be read. */
function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * Whether one value is a refusal's shape: a variant's name, or one variant
 * carrying its fields.
 *
 * The variant's name is not held to the names this client was generated with:
 * a supervisor newer than this client refuses for reasons it does not know,
 * and that refusal still reaches the caller as one rather than as an
 * unreadable answer.
 */
function isRefusal(refused: unknown): boolean {
  if (typeof refused === "string") {
    return refused !== "";
  }
  if (!isObject(refused)) {
    return false;
  }
  const arms = Object.values(refused);
  // llmlint: ignore[boundary_inputs_validated] See suppressions.toml.
  return arms.length === 1 && isObject(arms[0]);
}

/** A refusal's value asked for and range allowed, when it ruled on a value; `null` when its bounds are malformed. */
function boundsOf(refused: unknown): { requested?: number; allowed?: Range } | null {
  if (!isObject(refused) || !("out_of_bounds" in refused)) {
    return {};
  }
  const bounds = refused.out_of_bounds;
  if (!isObject(bounds)) {
    return null;
  }
  // An action's bounds name the adjustable they ruled on; a recovery's, its field.
  const { adjustable, field, requested, allowed } = bounds;
  const ruledOn = field ?? adjustable;
  if (typeof ruledOn !== "string" || typeof requested !== "number" || !isObject(allowed)) {
    return null;
  }
  const { min, max } = allowed;
  if (typeof min !== "number" || typeof max !== "number") {
    return null;
  }
  return { requested, allowed: { min, max } };
}

/** The policy's own refusal, as the answer to it carries it. */
function rejection(status: number, said: string): Error {
  const answer: unknown = document<unknown>(status, said);
  if (!isObject(answer)) {
    return new Unreadable(status, "the supervisor refused this action and answered no object");
  }
  // An action's answer carries its decision in its record; a recovery's and a
  // spool command's carry it at the top.
  const decision = isObject(answer.record) ? answer.record.decision : answer.decision;
  const refused = isObject(decision) ? decision.rejected : undefined;
  if (!isRefusal(refused)) {
    return new Unreadable(
      status,
      "the supervisor refused this action and answered a record whose decision is not a refusal",
    );
  }
  const bounds = boundsOf(refused);
  if (bounds === null) {
    return new Unreadable(status, "the refusal's bounds are not a value and a range");
  }
  return new Rejected(
    // The shape is checked above; the variant is passed through as `isRefusal`
    // says why, so the type is the contract's own name for a refusal.
    refused as RejectionReason | RecoveryRejection,
    bounds.requested,
    bounds.allowed,
    // An object whose decision is a refusal: the answer the contract names.
    answer as unknown as ActionAnswer | SpoolCommandAnswer | RecoveryAnswer,
  );
}

/** The machine's own refusal of an action the policy accepted. */
function machineRefusal(status: number, said: string): Error {
  const answered: unknown = document<unknown>(status, said);
  if (!isObject(answered)) {
    return new Unreadable(status, "the supervisor answered a refusal that is not an object");
  }
  // A recovery the machine refused carries no action record, and arrives as
  // the supervisor's own words for it.
  if (!("record" in answered)) {
    if (typeof answered.detail !== "string") {
      return new Unreadable(status, "the supervisor answered a refusal saying nothing");
    }
    return new Refused(status, answered.detail);
  }
  return new PrinterRefused(
    typeof answered.printer_refusal === "string"
      ? answered.printer_refusal
      : "it said nothing this client can read",
    // An object carrying an action record: the answer the contract names.
    answered as unknown as ActionAnswer,
  );
}
