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
import {
  GeneratedClient,
  RECOVERY_REJECTION_FIELDS,
  REJECTION_REASON_FIELDS,
  type RefusalField,
} from "./contract.ts";
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

/** Whether one parsed value is a finite number. */
function isFiniteNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

/** Whether one parsed value is an object of exactly one member: a decision's one arm. */
function isOneArm(value: unknown): value is Record<string, unknown> {
  return isObject(value) && Object.keys(value).length === 1;
}

/** Whether one parsed value is a JSON object, whose members can then be read. */
function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Whether one parsed value takes one JSON form a refusal's field declares. */
function takes(value: unknown, kind: string): boolean {
  switch (kind) {
    case "string":
      return typeof value === "string";
    case "number":
      return typeof value === "number" && Number.isFinite(value);
    case "integer":
      return Number.isInteger(value);
    case "boolean":
      return typeof value === "boolean";
    case "object":
      return isObject(value);
    case "array":
      return Array.isArray(value);
    case "string_or_object":
      return typeof value === "string" || isObject(value);
    default:
      return true;
  }
}

/**
 * Whether one variant's fields carry every field its table declares, each in
 * its own form.
 *
 * A field is held to its JSON form and not to the closed set of words or arms
 * the client was generated with, for the reason `isRefusal` gives.
 */
function carries(
  fields: Record<string, unknown>,
  declared: Readonly<Record<string, RefusalField>>,
): boolean {
  return Object.entries(declared).every(([name, [kind, required, nullable]]) => {
    const value = fields[name];
    if (value === undefined) {
      return !required;
    }
    if (value === null) {
      return nullable;
    }
    // llmlint: ignore[boundary_inputs_validated] See suppressions.toml.
    return takes(value, kind);
  });
}

/**
 * Whether one value is a refusal of one vocabulary: a variant's name, or one
 * variant carrying its fields.
 *
 * A variant the vocabulary's generated table names is held to it — named
 * alone when it carries nothing, and carrying each field it declares in that
 * field's own form when it carries any. A variant the table does not name is
 * a newer supervisor's, whose reason this client does not know; it still
 * reaches the caller as a refusal rather than as an unreadable answer.
 */
function isRefusal(
  refused: unknown,
  known: Readonly<Record<string, Readonly<Record<string, RefusalField>> | null>>,
): boolean {
  if (typeof refused === "string") {
    // llmlint: ignore[boundary_inputs_validated] See suppressions.toml.
    return refused !== "" && (!Object.hasOwn(known, refused) || known[refused] === null);
  }
  if (!isObject(refused)) {
    return false;
  }
  const arms = Object.entries(refused);
  const [arm] = arms;
  if (arms.length !== 1 || arm === undefined || !isObject(arm[1])) {
    return false;
  }
  const [name, fields] = arm;
  // llmlint: ignore[boundary_inputs_validated] See suppressions.toml.
  if (!Object.hasOwn(known, name)) {
    return true;
  }
  const declared = known[name];
  return declared !== null && declared !== undefined && carries(fields, declared);
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
  // `JSON.parse` reads a literal past any float as `Infinity`, which no bound is.
  if (typeof ruledOn !== "string" || !isFiniteNumber(requested) || !isObject(allowed)) {
    return null;
  }
  const { min, max } = allowed;
  if (!isFiniteNumber(min) || !isFiniteNumber(max)) {
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
  // spool command's carry it at the top. A recovery's names its action beside
  // its decision, and is refused in a recovery's own vocabulary.
  const decision = isObject(answer.record) ? answer.record.decision : answer.decision;
  // A decision is one arm: one carrying a refusal beside anything else is no
  // decision.
  const refused = isOneArm(decision) ? decision.rejected : undefined;
  // A recovery's answer is the one naming an action beside its decision, and
  // carries the action's own name, the supervisor's words and its events.
  const recovers = !isObject(answer.record) && "action" in answer;
  // llmlint: ignore[boundary_inputs_validated] See suppressions.toml.
  if (
    recovers &&
    !(
      isObject(answer.action) &&
      typeof answer.action.action === "string" &&
      typeof answer.detail === "string" &&
      Array.isArray(answer.events)
    )
  ) {
    return new Unreadable(status, "the supervisor answered a recovery's refusal missing its parts");
  }
  const known = recovers ? RECOVERY_REJECTION_FIELDS : REJECTION_REASON_FIELDS;
  if (!isRefusal(refused, known)) {
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
    // The shape and a known variant's fields are checked above; an unknown
    // variant is passed through as `isRefusal` says why, so the type is the
    // contract's own name for a refusal.
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
  // the supervisor's own words for it: the action it was, and a decision
  // carried out and failed at the printer, which is what a machine refusal is.
  if (!isObject(answered.record)) {
    const { action, decision, detail } = answered;
    const executed = isOneArm(decision) ? decision.executed : undefined;
    const failed = isOneArm(executed) ? executed.failed : undefined;
    if (
      !isObject(action) ||
      typeof action.action !== "string" ||
      !isObject(failed) ||
      typeof failed.reason !== "string" ||
      typeof detail !== "string"
    ) {
      return new Unreadable(
        status,
        "the supervisor answered a refusal that is neither an action's nor a failed recovery's",
      );
    }
    return new Refused(status, detail);
  }
  return new PrinterRefused(
    typeof answered.printer_refusal === "string"
      ? answered.printer_refusal
      : "it said nothing this client can read",
    // An object carrying an action record: the answer the contract names.
    answered as unknown as ActionAnswer,
  );
}
