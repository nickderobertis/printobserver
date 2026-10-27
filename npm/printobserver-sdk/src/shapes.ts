/**
 * Whether a parsed document is of one declared type of the contracts.
 *
 * What a type is comes from `SHAPES`, the table generated from the same
 * declarations as every type in `contract.ts`; this module only walks it. An
 * object admits members it does not declare, as the Rust client's types do,
 * and an optional member sent as null reads as one left out. Every closed set
 * admits only what it declares: an enumeration's names, a union's arms, a
 * tagged object's tags.
 */

import { SHAPES } from "./contract.ts";

/** Whether one parsed value is a JSON object, whose members can then be read. */
function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Whether `value` is a document of the type the contracts declare as `declared`. */
export function conforms(value: unknown, declared: string): boolean {
  return isDeclared(value, SHAPES[declared]);
}

/** Whether `value` is of one type expression of the table. */
function isOf(value: unknown, shape: unknown): boolean {
  if (!isObject(shape)) {
    return false;
  }
  if (typeof shape.ref === "string") {
    return isDeclared(value, SHAPES[shape.ref]);
  }
  if (typeof shape.scalar === "string") {
    return isScalar(value, shape.scalar);
  }
  if ("list" in shape) {
    return (
      Array.isArray(value) &&
      (value.length > 0 || shape.non_empty !== true) &&
      value.every((entry) => isOf(entry, shape.list))
    );
  }
  if ("map" in shape) {
    return isObject(value) && Object.values(value).every((entry) => isOf(entry, shape.map));
  }
  if ("nullable" in shape) {
    return value === null || isOf(value, shape.nullable);
  }
  return shape.any === true;
}

/** Whether `value` is one scalar of JSON: a number is finite. */
function isScalar(value: unknown, kind: string): boolean {
  switch (kind) {
    case "string":
      return typeof value === "string";
    case "null":
      return value === null;
    case "boolean":
      return typeof value === "boolean";
    case "integer":
      return Number.isInteger(value);
    case "number":
      return typeof value === "number" && Number.isFinite(value);
    default:
      return false;
  }
}

/** Whether `value` is of one declaration of the table. */
function isDeclared(value: unknown, declaration: unknown): boolean {
  if (!isObject(declaration)) {
    return false;
  }
  if ("alias" in declaration) {
    return isOf(value, declaration.alias);
  }
  if (Array.isArray(declaration.enum)) {
    return typeof value === "string" && declaration.enum.includes(value);
  }
  const { struct, union } = declaration;
  if (isObject(struct) && isObject(struct.fields) && isObject(struct.variants)) {
    if (!(isObject(value) && hasFields(value, struct.fields))) {
      return false;
    }
    return !struct.tag_field || isTagged(value, String(struct.tag_field), struct.variants);
  }
  if (isObject(union) && isObject(union.variants)) {
    if (typeof union.tag_field === "string" && union.tag_field !== "") {
      return isObject(value) && isTagged(value, union.tag_field, union.variants);
    }
    return isExternal(value, union.variants);
  }
  return false;
}

/** Whether a flag and the sibling it is held in step with agree. */
function isPaired(flag: unknown, value: Record<string, unknown>, pair: string): boolean {
  const sent = Object.hasOwn(value, pair);
  if (sent && value[pair] === null) {
    return false;
  }
  return typeof flag === "boolean" && flag !== sent;
}

/**
 * Whether an object carries each member its declaration requires, each of its
 * type. A member paired with an optional sibling is false exactly when that
 * sibling is sent, and the sibling is never sent as null.
 */
function hasFields(value: Record<string, unknown>, fields: Record<string, unknown>): boolean {
  return Object.entries(fields).every(([name, field]) => {
    if (!isObject(field) || typeof field.required !== "boolean") {
      return false;
    }
    if (typeof field.paired_with === "string" && field.paired_with !== "") {
      if (!isPaired(value[name], value, field.paired_with)) {
        return false;
      }
    }
    const member = value[name];
    if (member === undefined) {
      return !field.required;
    }
    if (member === null && !field.required) {
      return true;
    }
    return isOf(member, field.shape);
  });
}

/** Whether an object names one declared arm in `tagField`, and carries that arm. */
function isTagged(
  value: Record<string, unknown>,
  tagField: string,
  variants: Record<string, unknown>,
): boolean {
  const tag = value[tagField];
  if (typeof tag !== "string" || !Object.hasOwn(variants, tag)) {
    return false;
  }
  return isArm(value, variants[tag]);
}

/**
 * Whether a value is one arm of an externally tagged union: an arm carrying
 * nothing travels as its own name, and any other as an object of exactly one
 * member, its name, holding what it carries.
 */
function isExternal(value: unknown, variants: Record<string, unknown>): boolean {
  if (typeof value === "string") {
    const variant = Object.hasOwn(variants, value) ? variants[value] : undefined;
    return isObject(variant) && variant.unit === true;
  }
  if (!isObject(value)) {
    return false;
  }
  const entries = Object.entries(value);
  const [entry] = entries;
  if (entries.length !== 1 || entry === undefined) {
    return false;
  }
  const [tag, carried] = entry;
  const variant = Object.hasOwn(variants, tag) ? variants[tag] : undefined;
  if (!isObject(variant) || variant.unit !== false) {
    return false;
  }
  if (isObject(variant.payload)) {
    return isOf(carried, variant.payload);
  }
  return isObject(carried) && isObject(variant.fields) && hasFields(carried, variant.fields);
}

/** Whether an object carries one arm of an internally tagged set beside its tag. */
function isArm(value: Record<string, unknown>, variant: unknown): boolean {
  if (!isObject(variant)) {
    return false;
  }
  if (isObject(variant.payload)) {
    return isOf(value, variant.payload);
  }
  return isObject(variant.fields) && hasFields(value, variant.fields);
}
