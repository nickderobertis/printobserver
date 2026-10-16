// The two narrowings every module here reads a deserialized value through.

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
