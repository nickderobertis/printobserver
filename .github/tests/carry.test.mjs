// The two Linux jobs carrying `gate (macos-aarch64)` and
// `integration (macos-aarch64)`, driven as `ci.yml` runs them: the committed
// command line handed the `toJSON(needs)` the forge would hand it. The job
// results are the one thing doubled, because only the forge produces them.

import assert from "node:assert/strict";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, test } from "node:test";
import { policy } from "./support/histories.mjs";

/** `toJSON(needs)` as the forge renders it for the carrier jobs. */
function needs({
  classify = "success",
  required = "true",
  reason = "macOS required: an ordinary pull request",
  macos = "success",
  gate = "success",
  integration = "success",
} = {}) {
  return {
    classify: {
      result: classify,
      outputs: classify === "success" ? { "macos-required": required, reason } : {},
    },
    macos: {
      result: macos,
      outputs: macos === "skipped" || macos === "cancelled" ? {} : { gate, integration },
    },
  };
}

function carried(lane, given) {
  const summary = join(mkdtempSync(join(tmpdir(), "carry-")), "summary.md");
  const ran = policy(["carry", "--lane", lane], {
    env: {
      NEEDS: typeof given === "string" ? given : JSON.stringify(given),
      GITHUB_STEP_SUMMARY: summary,
    },
  });
  let written = "";
  try {
    written = readFileSync(summary, "utf8");
  } catch {
    written = "";
  }
  return { ...ran, summary: written };
}

describe("a macOS lane that ran", () => {
  for (const lane of ["gate", "integration"]) {
    test(`${lane} passes when its own step succeeded, and says it ran on macOS`, () => {
      const ran = carried(lane, needs());
      assert.equal(ran.status, 0);
      assert.match(ran.stdout, new RegExp(`ran its ${lane} on macos-aarch64 and it succeeded`));
      assert.match(ran.summary, /^PASS: /);
    });

    test(`${lane} fails when its own step failed`, () => {
      const ran = carried(lane, needs({ macos: "failure", [lane]: "failure" }));
      assert.equal(ran.status, 1);
      assert.match(ran.stdout, new RegExp(`${lane} ended failure`));
      assert.match(ran.summary, /^FAIL: /);
    });

    test(`${lane} fails when its own step never ran because a prerequisite failed`, () => {
      const ran = carried(lane, needs({ macos: "failure", [lane]: "skipped" }));
      assert.equal(ran.status, 1);
      assert.match(ran.stdout, new RegExp(`${lane} ended skipped`));
    });

    test(`${lane} fails when the lane was cancelled`, () => {
      const ran = carried(lane, needs({ macos: "cancelled" }));
      assert.equal(ran.status, 1);
      assert.match(ran.stdout, /the macOS lane was cancelled/);
    });

    test(`${lane} fails when the lane was owed and skipped`, () => {
      const ran = carried(lane, needs({ macos: "skipped" }));
      assert.equal(ran.status, 1);
      assert.match(ran.stdout, /the macOS lane was skipped/);
    });
  }

  test("a failed gate does not fail the integration context, which judges its own lane", () => {
    const given = needs({ macos: "failure", gate: "failure", integration: "success" });
    assert.equal(carried("gate", given).status, 1);
    assert.equal(carried("integration", given).status, 0);
  });

  test("a lane that reported no outcome fails rather than passing", () => {
    const given = needs();
    given.macos.outputs = {};
    const ran = carried("gate", given);
    assert.equal(ran.status, 1);
    assert.match(ran.stdout, /reported no outcome/);
  });
});

describe("an omission the classifier authorized", () => {
  test("passes, saying no macOS test ran", () => {
    const reason = "release branch `release-plz-x` of this repository changes only version fields";
    const ran = carried("gate", needs({ required: "false", reason, macos: "skipped" }));
    assert.equal(ran.status, 0);
    assert.match(ran.stdout, /no macOS gate ran on this run, and none was owed/);
    assert.match(ran.stdout, /authorized omission, not a macOS pass/);
    assert.ok(ran.stdout.includes(reason));
  });

  test("a lane that ran anyway is judged on what it did", () => {
    const ran = carried(
      "integration",
      needs({ required: "false", macos: "failure", integration: "failure" }),
    );
    assert.equal(ran.status, 1);
  });
});

describe("a classification that cannot be trusted", () => {
  test("a failed classifier fails both contexts, whatever the lane did", () => {
    for (const lane of ["gate", "integration"]) {
      const ran = carried(lane, needs({ classify: "failure", macos: "success" }));
      assert.equal(ran.status, 1);
      assert.match(ran.stdout, /the classifier did not succeed \(failure\)/);
    }
  });

  test("a cancelled classifier fails", () => {
    assert.equal(carried("gate", needs({ classify: "cancelled", macos: "skipped" })).status, 1);
  });

  test("an answer that is neither true nor false fails", () => {
    const ran = carried("gate", needs({ required: "", macos: "skipped" }));
    assert.equal(ran.status, 1);
    assert.match(ran.stdout, /neither true nor false/);
  });

  test("results the forge did not hand over fail", () => {
    assert.equal(carried("gate", "").status, 1);
    assert.equal(carried("gate", { classify: needs().classify }).status, 1);
  });

  test("a lane the macOS job does not carry is refused", () => {
    const ran = carried("llmlint", needs());
    assert.equal(ran.status, 1);
    assert.match(ran.stdout, /not a lane the macOS job carries/);
  });
});
