/**
 * What every generated method is written against, and every way a call can end.
 *
 * The generated module carries one method per operation the server declares.
 * This is what those methods stand on: the two things a call needs from the
 * client — who it acts as, and how to make a request — and the closed set of
 * failures a caller distinguishes.
 *
 * Nothing here imports the generated module at run time. The type names below
 * are that module's, imported as types alone, so that the two files can name
 * each other's shapes without either being unloadable.
 */

import type { ActionAnswer, Actor, Range, RejectionReason } from "./contract.ts";

/** Why a call did not answer what it asked for. */
export class ClientError extends Error {
  constructor(said: string) {
    super(said);
    this.name = "ClientError";
  }
}

/** Nothing answered at the configured address. */
export class Unreachable extends ClientError {
  /** Where this client looked. */
  readonly address: string;
  /** What happened there, in the words of whatever failed. */
  readonly detail: string;

  constructor(address: string, detail: string) {
    super(
      `nothing answered at ${address}: ${detail}. Start the supervisor there, or point ` +
        `this client at the address it is answering on`,
    );
    this.name = "Unreachable";
    this.address = address;
    this.detail = detail;
  }
}

/**
 * This client would not send the request, because it carried no reason.
 *
 * Every mutating operation carries a reason, and a call whose reason is empty
 * is refused here rather than at the server: nothing reaches the policy, the
 * printer or the record.
 */
export class NoReason extends ClientError {
  constructor() {
    super("every mutating call carries a reason, and this one carries none");
    this.name = "NoReason";
  }
}

/** The supervisor answered something this client cannot read. */
export class Unreadable extends ClientError {
  /** The status it answered under. */
  readonly status: number;
  /** What could not be read, in the words of whatever refused it. */
  readonly detail: string;

  constructor(status: number, detail: string) {
    super(`the supervisor answered ${status} with something this client cannot read: ${detail}`);
    this.name = "Unreadable";
    this.status = status;
    this.detail = detail;
  }
}

/** The supervisor will not do what it was asked, and said why. */
export class Refused extends ClientError {
  /** The status it answered under. */
  readonly status: number;
  /** What it said, in its own words. */
  readonly detail: string;

  constructor(status: number, detail: string) {
    super(`the supervisor answered ${status}: ${detail}`);
    this.name = "Refused";
    this.status = status;
    this.detail = detail;
  }
}

/** The policy accepted the action and the machine refused it. */
export class PrinterRefused extends ClientError {
  /** What the printer said. */
  readonly detail: string;
  /** The record of the request and the decision taken on it. */
  readonly answer: ActionAnswer;

  constructor(detail: string, answer: ActionAnswer) {
    super(
      `the policy accepted this action and the machine refused it: ${detail}. Look at ` +
        `the printer, then ask again`,
    );
    this.name = "PrinterRefused";
    this.detail = detail;
    this.answer = answer;
  }
}

/**
 * The policy refused the action, and the refusal is the answer.
 *
 * `reason` is the refusal itself, matched rather than read; `requested` and
 * `allowed` are the value that was asked for and the range that is allowed,
 * which the policy states when it ruled on a value and which are undefined
 * when it ruled on something else.
 */
export class Rejected extends ClientError {
  /** Why the action was refused. */
  readonly reason: RejectionReason;
  /** The value that was asked for, where the policy ruled on one. */
  readonly requested: number | undefined;
  /** The range that is allowed, where the policy ruled on a value. */
  readonly allowed: Range | undefined;
  /** The whole answer, whose record carries the request and the decision. */
  readonly answer: ActionAnswer;

  constructor(
    reason: RejectionReason,
    requested: number | undefined,
    allowed: Range | undefined,
    answer: ActionAnswer,
  ) {
    super(
      `the supervisor's policy refused this action: ${JSON.stringify(reason)}` +
        (requested !== undefined && allowed !== undefined
          ? `. It was asked for ${requested}, and what is allowed is ${allowed.min} to ${allowed.max}`
          : ""),
    );
    this.name = "Rejected";
    this.reason = reason;
    this.requested = requested;
    this.allowed = allowed;
    this.answer = answer;
  }
}

/**
 * Refuse a mutating call that carries no reason, before it is made.
 *
 * @throws {NoReason} If the reason is empty or is nothing but whitespace.
 */
export function reasonGiven(reason: string): void {
  if (reason.trim() === "") {
    throw new NoReason();
  }
}

/**
 * The two things every generated method needs from the client it is on.
 *
 * A surface on its own makes no request: `Client` is what implements `call`,
 * and the generated methods are written against this so that the transport and
 * the operation list stay separable.
 *
 * It is a plain class rather than an abstract one, and deliberately: this
 * repository measures line coverage on the Node client, and the coverage
 * reporter maps no line of a file carrying an abstract member — a file this
 * suite drives every branch of would report as one line covered.
 */
export class GeneratedSurface {
  /** Who this client acts as, which every mutating call carries. */
  readonly actor: Actor;

  constructor(actor: Actor) {
    this.actor = actor;
  }

  /**
   * Make one call and answer the document that came back.
   *
   * @throws {Error} Always. `Client` is what makes a request; a surface that
   * answered one would be a second transport.
   */
  protected call<T>(
    method: string,
    target: string,
    asked: Array<[string, string]>,
    sending: Record<string, unknown> | undefined,
  ): Promise<T> {
    throw new Error(
      `a surface makes no request: ${method} ${target} with ${asked.length} asked ` +
        `for and ${sending === undefined ? "no" : "a"} body`,
    );
  }
}
