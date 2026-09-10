/**
 * A stub supervisor on a real socket, for the generated walk to drive.
 *
 * It is a **host** rather than a stand-in for the client's transport: the
 * client opens a real connection, writes a real request and reads a real
 * answer, and what this records is what arrived over that connection. The
 * layer under test is the client, and nothing here is inside it.
 *
 * The real server is what the printer integration tier drives. This is what the
 * fast tier drives, because a walk over every operation needs an answer of
 * every shape and a real supervisor cannot be put into sixteen states in a
 * second.
 */

/** What arrived over one connection. */
export interface Received {
  /** The method it was made by. */
  method: string;
  /** The whole request target, question mark and all. */
  target: string;
  /** The body it carried, empty where it carried none. */
  body: string;
}

/** The media type every operation answers in. */
const MEDIA_TYPE = "application/json";

/** A host answering one canned document to whatever it is asked. */
export class Host {
  private readonly arrivals: Received[] = [];
  private readonly server: ReturnType<typeof Bun.serve>;

  private constructor(status: number, answer: unknown) {
    const arrivals = this.arrivals;
    const said = JSON.stringify(answer);
    this.server = Bun.serve({
      port: 0,
      hostname: "127.0.0.1",
      async fetch(request: Request): Promise<Response> {
        const target = new URL(request.url);
        arrivals.push({
          method: request.method,
          target: `${target.pathname}${target.search}`,
          body: await request.text(),
        });
        return new Response(said, {
          status,
          headers: { "Content-Type": MEDIA_TYPE },
        });
      },
    });
  }

  /** A host answering `status` with `answer`, on a port the system chooses. */
  static answering(status: number, answer: unknown): Host {
    return new Host(status, answer);
  }

  /** Where this host answers. */
  get address(): string {
    return `http://127.0.0.1:${this.server.port}`;
  }

  /**
   * What arrived over the one connection this host served.
   *
   * @throws {Error} If nothing arrived, which is a call that was never made.
   */
  received(): Received {
    const first = this.arrivals[0];
    if (first === undefined) {
      throw new Error("no call reached this host");
    }
    return first;
  }

  /**
   * How many requests reached this host.
   *
   * What a caller asks this is whether a call that should have been refused
   * before it was made reached the wire.
   */
  requests(): number {
    return this.arrivals.length;
  }

  /** Stop serving, leaving no listener behind. */
  async [Symbol.asyncDispose](): Promise<void> {
    await this.server.stop(true);
  }
}
