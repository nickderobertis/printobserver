import { expect, test } from "bun:test";
import { exportedNames } from "../src/index.ts";

test("the package imports and exports nothing yet", () => {
  expect(exportedNames).toEqual([]);
});
