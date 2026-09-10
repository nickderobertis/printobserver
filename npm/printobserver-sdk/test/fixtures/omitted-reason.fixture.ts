/**
 * A call that leaves the reason out, which must not compile.
 *
 * Every mutating method takes the reason as an argument, so omitting it is a
 * compile-time error and produces no run-time refusal a journey could inspect.
 * This file is what makes that a check rather than a claim: `compilation.test.ts`
 * runs the type checker over it and asserts it is refused.
 *
 * It is deliberately outside this package's own `tsconfig.json`, because a
 * fixture that must not compile cannot be part of a type check that must pass.
 */

import { Client } from "../../src/client.ts";

const client = new Client({ server: "http://127.0.0.1:8420", actor: "operator" });

// No reason: this does not compile, which is the point.
await client.pause("0198f0a1-2b3c-7d4e-8f90-123456789abc");
