// The narrowings every module here reads a deserialized value through, and the
// one rule every module counts a macOS runner by.

/**
 * @param {unknown} value
 * @returns {value is Record<string, any>}
 */
export function isObject(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * @param {unknown} error
 * @returns {string}
 */
export function message(error) {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Whether a runner label names a macOS runner: `macos-15`, `macos-latest`, or a
 * self-hosted label set's `macOS`.
 *
 * @param {string} label
 * @returns {boolean}
 */
export function isMacosLabel(label) {
  return /(^|-)macos(-|$)/i.test(label);
}
