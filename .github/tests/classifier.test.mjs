// The release-only classifier, driven as the `classify` job drives it: the
// committed command line over a real event payload and a generated history.

import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { describe, test } from "node:test";
import { classified, fixture, git, history, outputsOf, policy } from "./support/histories.mjs";
import { pullRequestPayload } from "./support/lifecycle.mjs";

const RELEASE_ONLY = { isReleaseOnly: true, macosRequired: false };
const OWED = { isReleaseOnly: false, macosRequired: true };
const NOT_A_PULL_REQUEST = { isReleaseOnly: false, macosRequired: false };

function verdict(result) {
  return { isReleaseOnly: result.isReleaseOnly, macosRequired: result.macosRequired };
}

describe("release automation's own pull request", () => {
  test("a version bump with its changelog is release-only, whoever authored it", () => {
    const made = history("version");
    const payload = pullRequestPayload("opened", "release", "same-repository", made);
    assert.equal(
      payload.pull_request.user.login,
      "nickderobertis",
      "release-plz opens under a person's token",
    );
    const result = classified("pull_request", payload, made);
    assert.deepEqual(verdict(result), RELEASE_ONLY);
    assert.match(
      result.reason,
      /Cargo\.lock, Cargo\.toml, crates\/printobserver-core\/CHANGELOG\.md/,
    );
  });

  test("a changelog alone is release-only", () => {
    const made = history("changelog");
    const result = classified(
      "pull_request",
      pullRequestPayload("synchronize", "release", "same-repository", made),
      made,
    );
    assert.deepEqual(verdict(result), RELEASE_ONLY);
  });

  test("the same author on an ordinary branch owes macOS: the author is never the authority", () => {
    const made = history("version");
    const payload = pullRequestPayload("opened", "ordinary", "same-repository", made);
    payload.pull_request.user.login = "nickderobertis";
    payload.pull_request.title = "chore: release v9.9.9";
    const result = classified("pull_request", payload, made);
    assert.deepEqual(verdict(result), OWED);
    assert.match(result.reason, /ordinary pull request/);
  });

  test("a bot author on an ordinary branch owes macOS", () => {
    const made = history("version");
    const payload = pullRequestPayload("opened", "ordinary", "same-repository", made);
    payload.pull_request.user = { login: "github-actions[bot]", type: "Bot" };
    payload.sender = payload.pull_request.user;
    assert.deepEqual(verdict(classified("pull_request", payload, made)), OWED);
  });
});

describe("what a release pull request may carry, read semantically", () => {
  for (const diff of ["changelog-added", "crate-path-version", "lock-qualified"]) {
    test(`a ${diff} release is release-only`, () => {
      const made = history(diff);
      const result = classified(
        "pull_request",
        pullRequestPayload("opened", "release", "same-repository", made),
        made,
      );
      assert.deepEqual(verdict(result), RELEASE_ONLY, result.reason);
    });
  }

  for (const [diff, why] of /** @type {[string, RegExp][]} */ ([
    ["empty", /changes nothing, which no release does/],
    ["changelog-deleted", /deletes changelog crates\/printobserver-core\/CHANGELOG\.md/],
    [
      "lock-version-garbled",
      /Cargo\.lock in a way that is not a version: printobserver-core's version is "garbled"/,
    ],
    ["config-garbled", /release-plz\.toml at [0-9a-f]+ is not TOML/],
    ["config-no-prefix", /release-plz\.toml at [0-9a-f]+ names no \[workspace\] pr_branch_prefix/],
  ])) {
    test(`a ${diff} release owes macOS`, () => {
      const made = history(diff);
      const result = classified(
        "pull_request",
        pullRequestPayload("opened", "release", "same-repository", made),
        made,
      );
      assert.deepEqual(verdict(result), OWED);
      assert.match(result.reason, why);
    });
  }
});

describe("forged and mixed release pull requests", () => {
  test("a fork's branch named like release-plz's owes macOS", () => {
    const made = history("version");
    const result = classified(
      "pull_request",
      pullRequestPayload("opened", "release", "fork", made),
      made,
    );
    assert.deepEqual(verdict(result), OWED);
    assert.match(result.reason, /a fork's branch/);
  });

  test("a head repository claiming to be no fork while naming another repository owes macOS", () => {
    const made = history("version");
    const payload = pullRequestPayload("opened", "release", "same-repository", made);
    payload.pull_request.head.repo = {
      ...payload.pull_request.head.repo,
      full_name: "someone/printobserver",
    };
    assert.deepEqual(verdict(classified("pull_request", payload, made)), OWED);
  });

  for (const [diff, why] of /** @type {[string, RegExp][]} */ ([
    ["source", /src\/lib\.rs, which is neither a changelog nor a version manifest/],
    ["mixed", /src\/lib\.rs/],
    ["dependency", /Cargo\.toml beyond its version fields \(workspace\.dependencies\.serde_json\)/],
    ["lockfile-source", /Cargo\.lock beyond its version fields/],
    ["version-garbled", /not a version/],
  ])) {
    test(`a release branch carrying a ${diff} change owes macOS`, () => {
      const made = history(diff);
      const result = classified(
        "pull_request",
        pullRequestPayload("opened", "release", "same-repository", made),
        made,
      );
      assert.deepEqual(verdict(result), OWED);
      assert.match(result.reason, why);
    });
  }

  test("a release branch that deletes a manifest owes macOS", () => {
    const made = history("version");
    git(["rm", "-q", "crates/printobserver-types/Cargo.toml"], made.dir);
    git(["commit", "-q", "-m", "delete"], made.dir);
    const head = git(["rev-parse", "HEAD"], made.dir);
    const result = classified(
      "pull_request",
      pullRequestPayload("opened", "release", "same-repository", { ...made, head }),
      { ...made, head },
    );
    assert.deepEqual(verdict(result), OWED);
    assert.match(result.reason, /deletes crates\/printobserver-types\/Cargo\.toml/);
  });

  test("a prefix moved by the pull request itself is not the prefix it is judged by", () => {
    const made = history("version");
    const prefix = "feature/";
    const config = join(made.dir, "release-plz.toml");
    writeFileSync(config, `[workspace]\npr_branch_prefix = "${prefix}"\n`);
    git(["commit", "-q", "-am", "move the prefix"], made.dir);
    const head = git(["rev-parse", "HEAD"], made.dir);
    const payload = pullRequestPayload("opened", "ordinary", "same-repository", { ...made, head });
    assert.deepEqual(verdict(classified("pull_request", payload, { ...made, head })), OWED);
  });
});

describe("input the classifier cannot read owes macOS", () => {
  test("an event file that is not JSON", () => {
    const made = history("version");
    const path = join(made.dir, "garbled.json");
    writeFileSync(path, "{ not json");
    const ran = policy(["classify", "--event", path, "--base", made.base, "--head", made.head], {
      cwd: made.dir,
      env: { GITHUB_EVENT_NAME: "pull_request" },
    });
    assert.equal(ran.status, 0);
    const result = JSON.parse(ran.stdout);
    assert.deepEqual(verdict(result), OWED);
    assert.match(result.reason, /could not be read/);
  });

  test("a pull request event with no head repository", () => {
    const made = history("version");
    const payload = pullRequestPayload("opened", "release", "same-repository", made);
    delete payload.pull_request.head.repo;
    assert.deepEqual(verdict(classified("pull_request", payload, made)), OWED);
  });

  test("a base commit the clone does not carry", () => {
    const made = history("version");
    const missing = { ...made, base: "1".repeat(40) };
    const result = classified(
      "pull_request",
      pullRequestPayload("opened", "release", "same-repository", missing),
      missing,
    );
    assert.deepEqual(verdict(result), OWED);
    assert.match(result.reason, /git .* failed/);
  });

  test("an event the caller does not name and whose payload names none", () => {
    const made = history("version");
    const path = join(made.dir, "empty.json");
    writeFileSync(path, "{}");
    const ran = policy(["classify", "--event", path, "--base", made.base, "--head", made.head], {
      cwd: made.dir,
    });
    assert.deepEqual(verdict(JSON.parse(ran.stdout)), OWED);
  });
});

describe("no automatic run but a pull request's requests macOS", () => {
  for (const [eventName, name] of [
    ["push", "push.main"],
    ["push", "push.main.release-merge"],
    ["push", "push.tag"],
    ["release", "release.published"],
    ["workflow_run", "workflow_run.completed"],
    ["repository_dispatch", "repository_dispatch"],
    ["schedule", "schedule"],
    ["workflow_dispatch", "workflow_dispatch.macos"],
  ]) {
    test(`${name}`, () => {
      const made = history("source");
      const result = classified(eventName, fixture(name, made), made);
      assert.deepEqual(verdict(result), NOT_A_PULL_REQUEST);
      assert.match(result.reason, new RegExp(`a ${eventName} run is not a pull request`));
    });
  }

  test("a push whose event file cannot be read is still no pull request", () => {
    const ran = policy(
      ["classify", "--event", "/nonexistent/event.json", "--base", "HEAD", "--head", "HEAD"],
      {
        env: { GITHUB_EVENT_NAME: "push" },
      },
    );
    assert.deepEqual(verdict(JSON.parse(ran.stdout)), NOT_A_PULL_REQUEST);
  });

  test("the event name is read off the payload where the forge's variable is unset", () => {
    const made = history("version");
    const path = join(made.dir, "push.json");
    writeFileSync(path, JSON.stringify(fixture("push.main", made)));
    const ran = policy(["classify", "--event", path, "--base", made.base, "--head", made.head], {
      cwd: made.dir,
    });
    assert.deepEqual(verdict(JSON.parse(ran.stdout)), NOT_A_PULL_REQUEST);
  });
});

describe("the command line", () => {
  test("outputs prints the three step outputs and logs the classification", () => {
    const ran = policy(["outputs", "-"], {
      input: JSON.stringify({ isReleaseOnly: true, macosRequired: false, reason: "two\nlines" }),
    });
    assert.equal(ran.status, 0);
    assert.equal(ran.stdout, "macos-required=false\nreason=two lines\n");
    assert.match(ran.stderr, /"isReleaseOnly":true/);
    assert.deepEqual(outputsOf({ isReleaseOnly: false, macosRequired: true, reason: "r" }), {
      "macos-required": "true",
      reason: "r",
    });
  });

  test("outputs refuses what is not a classification", () => {
    const ran = policy(["outputs", "-"], { input: JSON.stringify({ macosRequired: "yes" }) });
    assert.equal(ran.status, 1);
    assert.match(
      ran.stderr,
      /not shaped like a classification.*hand `outputs` what `classify` printed/,
    );
  });

  for (const [args, why] of /** @type {[string[], RegExp][]} */ ([
    [[], /no command given/],
    [["classify", "--event", "e.json"], /missing --base, --head/],
    [
      ["classify", "--event", "e.json", "--base", "a", "--head", "b", "--extra", "x"],
      /`--extra` is not an option/,
    ],
    [
      ["classify", "--event", "e.json", "--event", "f.json", "--base", "a", "--head", "b"],
      /--event is given twice/,
    ],
    [["carry"], /missing --lane/],
    [["outputs"], /outputs takes one classification file/],
    [["deploy"], /`deploy` is not a command/],
  ])) {
    test(`refuses ${JSON.stringify(args)} with usage`, () => {
      const ran = policy(args);
      assert.equal(ran.status, 2);
      assert.match(ran.stderr, why);
      assert.match(ran.stderr, /usage: macos-policy\.mjs classify/);
    });
  }
});
