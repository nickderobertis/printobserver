/**
 * The one supervisor all three clients' printer-integration journeys drive.
 *
 * It is stood up by this repository's own tool rather than by each client in
 * its own language: a world built three times would be three worlds, and what
 * the three journeys are for is that the same nine steps against the same
 * supervisor come out the same in all three.
 */

import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";

/** Where the supervisor is, and what a journey acts on. */
export interface Supervisor {
  server: string;
  /** The credential it serves under, as the client configuration it wrote carries it. */
  credential: string;
  print_id: string;
  image_id: string;
  event_id: string;
  file_name: string;
}

/** The repository this package is in. */
const REPO_ROOT = resolve(dirname(new URL(import.meta.url).pathname), "../../..");

/**
 * Only the real-server setup hook's budget. Three warmed Linux runs measured
 * 2660, 3016 and 2737 ms, and a macOS runner overran ten seconds standing the
 * same world up, so what bounds it is what the world itself may do rather than
 * a warmed measurement. Both clients read the same committed budget because
 * the first time the tier is driven includes a release build of the program,
 * and this is the same world. Test bodies and cleanup keep their own timeouts.
 */
const startupTimeoutSeconds = Number(
  readFileSync(`${REPO_ROOT}/tools/release-artifacts/world-startup-timeout-seconds`, "utf8").trim(),
);
if (!Number.isSafeInteger(startupTimeoutSeconds) || startupTimeoutSeconds <= 0) {
  throw new Error("the shared world startup timeout must be a positive whole number of seconds");
}
export const SETUP_TIMEOUT_MS = startupTimeoutSeconds * 1000;

/** The packages this repository's own tools live in, read from the justfile. */
function pythonPath(): string {
  for (const line of readFileSync(`${REPO_ROOT}/justfile`, "utf8").split("\n")) {
    if (line.startsWith("export PYTHONPATH :=")) {
      return line.split(":=")[1]?.trim().replaceAll('"', "") ?? "";
    }
  }
  throw new Error("the justfile exports no PYTHONPATH, and this tier's world lives on it");
}

/** A supervisor held up for as long as this is alive. */
export class Standing {
  private readonly holding: ReturnType<typeof Bun.spawn>;
  readonly at: Supervisor;

  private constructor(holding: ReturnType<typeof Bun.spawn>, at: Supervisor) {
    this.holding = holding;
    this.at = at;
  }

  /** Bring one up under `into`, over the scripted OctoPrint. */
  static async standing(into: string): Promise<Standing> {
    const holding = Bun.spawn(
      [
        "uv",
        "run",
        "-q",
        "python",
        "-m",
        "release_artifacts",
        "world",
        "--octoprint",
        "--into",
        into,
      ],
      {
        cwd: REPO_ROOT,
        env: { ...process.env, PYTHONPATH: pythonPath() },
        stdin: "pipe",
        stdout: "pipe",
        stderr: "pipe",
      },
    );

    const said = await firstLine(holding.stdout);
    if (said.trim() === "") {
      const why = await new Response(holding.stderr).text();
      holding.kill();
      throw new Error(
        "the world did not come up. Run `just octoprint-up` first; this tier drives a " +
          `real OctoPrint and has no fixture to fall back to.\n${why}`,
      );
    }
    return new Standing(holding, JSON.parse(said) as Supervisor);
  }

  /** Close the input the world waits on, which is how it is asked to stop. */
  async stop(): Promise<void> {
    const input = this.holding.stdin;
    if (input !== undefined && typeof input !== "number") {
      input.end();
    }
    await this.holding.exited;
  }
}

/** The first line one stream carries. */
async function firstLine(stream: ReadableStream<Uint8Array>): Promise<string> {
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  let said = "";
  while (!said.includes("\n")) {
    const { done, value } = await reader.read();
    if (done) {
      break;
    }
    said += decoder.decode(value, { stream: true });
  }
  reader.releaseLock();
  return said.split("\n")[0] ?? "";
}
