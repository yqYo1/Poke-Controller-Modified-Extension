import openApiDocument from './api/openapi.json';

import type { components } from './api/openapi';

export type ClientMessage = components['schemas']['ClientMessage'];
export type ServerMessage = components['schemas']['ServerMessage'];
export type SettingsSnapshot = components['schemas']['SettingsSnapshot'];
export type StateSnapshot = components['schemas']['StateSnapshot'];

type JsonObject = Record<string, unknown>;

interface ObjectShape {
  readonly additionalSchemas: unknown[];
  closed: boolean;
  readonly properties: Map<string, unknown[]>;
  readonly required: Set<string>;
}

const openApi: unknown = openApiDocument;

function isObject(value: unknown): value is JsonObject {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function resolveReference(reference: string): unknown {
  if (!reference.startsWith('#/')) {
    return undefined;
  }

  let current = openApi;
  for (const encodedPart of reference.slice(2).split('/')) {
    if (!isObject(current)) {
      return undefined;
    }
    const part = encodedPart.replaceAll('~1', '/').replaceAll('~0', '~');
    current = current[part];
  }
  return current;
}

function schemaArray(value: unknown): unknown[] | undefined {
  return Array.isArray(value) ? value : undefined;
}

function stringArray(value: unknown): string[] | undefined {
  if (!Array.isArray(value) || !value.every((item) => typeof item === 'string')) {
    return undefined;
  }
  return value;
}

function matchesType(value: unknown, expected: string): boolean {
  switch (expected) {
    case 'array':
      return Array.isArray(value);
    case 'boolean':
      return typeof value === 'boolean';
    case 'integer':
      return typeof value === 'number' && Number.isFinite(value) && Number.isInteger(value);
    case 'null':
      return value === null;
    case 'number':
      return typeof value === 'number' && Number.isFinite(value);
    case 'object':
      return isObject(value);
    case 'string':
      return typeof value === 'string';
    default:
      return true;
  }
}

function addIssue(issues: string[], path: string, message: string): false {
  issues.push(`${path}: ${message}`);
  return false;
}

function mergeShape(target: ObjectShape, source: ObjectShape): void {
  target.closed ||= source.closed;
  for (const name of source.required) {
    target.required.add(name);
  }
  for (const [name, schemas] of source.properties) {
    const current = target.properties.get(name) ?? [];
    current.push(...schemas);
    target.properties.set(name, current);
  }
  target.additionalSchemas.push(...source.additionalSchemas);
}

function collectObjectShape(schema: unknown, depth = 0): ObjectShape | undefined {
  if (depth > 100 || !isObject(schema)) {
    return undefined;
  }
  if (typeof schema.$ref === 'string') {
    return collectObjectShape(resolveReference(schema.$ref), depth + 1);
  }

  const allOf = schemaArray(schema.allOf);
  const hasObjectKeywords =
    schema.type === 'object' ||
    isObject(schema.properties) ||
    Array.isArray(schema.required) ||
    schema.additionalProperties !== undefined;
  if (allOf === undefined && !hasObjectKeywords) {
    return undefined;
  }

  const shape: ObjectShape = {
    additionalSchemas: [],
    closed: schema.additionalProperties === false,
    properties: new Map(),
    required: new Set(stringArray(schema.required) ?? [])
  };

  if (isObject(schema.properties)) {
    for (const [name, propertySchema] of Object.entries(schema.properties)) {
      shape.properties.set(name, [propertySchema]);
    }
  }
  if (schema.additionalProperties !== undefined && schema.additionalProperties !== false) {
    if (schema.additionalProperties !== true) {
      shape.additionalSchemas.push(schema.additionalProperties);
    }
  }
  if (allOf !== undefined) {
    for (const member of allOf) {
      const memberShape = collectObjectShape(member, depth + 1);
      if (memberShape === undefined) {
        return undefined;
      }
      mergeShape(shape, memberShape);
    }
  }
  return shape;
}

function validateObject(
  value: JsonObject,
  shape: ObjectShape,
  path: string,
  issues: string[],
  depth: number
): boolean {
  let valid = true;
  for (const name of shape.required) {
    if (!Object.hasOwn(value, name)) {
      valid = addIssue(issues, path, `missing required property ${JSON.stringify(name)}`);
    }
  }

  for (const [name, propertyValue] of Object.entries(value)) {
    const propertyPath = `${path}.${name}`;
    const propertySchemas = shape.properties.get(name);
    if (propertySchemas !== undefined) {
      for (const propertySchema of propertySchemas) {
        valid = validateSchema(propertyValue, propertySchema, propertyPath, issues, depth + 1) && valid;
      }
      continue;
    }
    if (shape.closed) {
      valid = addIssue(issues, propertyPath, 'unknown property');
      continue;
    }
    for (const additionalSchema of shape.additionalSchemas) {
      valid =
        validateSchema(propertyValue, additionalSchema, propertyPath, issues, depth + 1) && valid;
    }
  }
  return valid;
}

function validateCombinator(
  value: unknown,
  variants: unknown[],
  exact: boolean,
  path: string,
  issues: string[],
  depth: number
): boolean {
  let matches = 0;
  for (const variant of variants) {
    const candidateIssues: string[] = [];
    if (validateSchema(value, variant, path, candidateIssues, depth + 1)) {
      matches += 1;
    }
  }
  const valid = exact ? matches === 1 : matches > 0;
  return valid || addIssue(issues, path, exact ? 'does not match exactly one variant' : 'does not match any variant');
}

function validateSchema(
  value: unknown,
  schema: unknown,
  path: string,
  issues: string[],
  depth: number
): boolean {
  if (depth > 100) {
    return addIssue(issues, path, 'schema nesting is too deep');
  }
  if (schema === true) {
    return true;
  }
  if (schema === false) {
    return addIssue(issues, path, 'value is forbidden');
  }
  if (!isObject(schema)) {
    return addIssue(issues, path, 'schema is unavailable');
  }
  if (typeof schema.$ref === 'string') {
    const resolved = resolveReference(schema.$ref);
    if (resolved === undefined) {
      return addIssue(issues, path, `unresolved schema reference ${schema.$ref}`);
    }
    return validateSchema(value, resolved, path, issues, depth + 1);
  }

  const oneOf = schemaArray(schema.oneOf);
  if (oneOf !== undefined && !validateCombinator(value, oneOf, true, path, issues, depth)) {
    return false;
  }
  const anyOf = schemaArray(schema.anyOf);
  if (anyOf !== undefined && !validateCombinator(value, anyOf, false, path, issues, depth)) {
    return false;
  }

  const allOf = schemaArray(schema.allOf);
  if (allOf !== undefined) {
    const shape = collectObjectShape(schema);
    if (shape !== undefined && isObject(value)) {
      if (!validateObject(value, shape, path, issues, depth)) {
        return false;
      }
    } else {
      for (const member of allOf) {
        if (!validateSchema(value, member, path, issues, depth + 1)) {
          return false;
        }
      }
    }
  }

  const expectedTypes =
    typeof schema.type === 'string'
      ? [schema.type]
      : stringArray(schema.type);
  if (
    expectedTypes !== undefined &&
    !expectedTypes.some((expected) => matchesType(value, expected))
  ) {
    return addIssue(issues, path, `expected ${expectedTypes.join(' or ')}`);
  }

  if (Array.isArray(schema.enum) && !schema.enum.some((item) => Object.is(item, value))) {
    return addIssue(issues, path, 'value is outside the closed enum');
  }
  if (Object.hasOwn(schema, 'const') && !Object.is(schema.const, value)) {
    return addIssue(issues, path, 'value does not equal the required constant');
  }

  if (typeof value === 'string') {
    const length = Array.from(value).length;
    if (typeof schema.minLength === 'number' && length < schema.minLength) {
      return addIssue(
        issues,
        path,
        `must contain at least ${String(schema.minLength)} characters`
      );
    }
    if (typeof schema.maxLength === 'number' && length > schema.maxLength) {
      return addIssue(
        issues,
        path,
        `must contain at most ${String(schema.maxLength)} characters`
      );
    }
    if (typeof schema.pattern === 'string' && !new RegExp(schema.pattern, 'u').test(value)) {
      return addIssue(issues, path, 'does not match the required pattern');
    }
  }

  if (typeof value === 'number') {
    if (typeof schema.minimum === 'number' && value < schema.minimum) {
      return addIssue(issues, path, `must be at least ${String(schema.minimum)}`);
    }
    if (typeof schema.maximum === 'number' && value > schema.maximum) {
      return addIssue(issues, path, `must be at most ${String(schema.maximum)}`);
    }
  }

  if (Array.isArray(value)) {
    if (typeof schema.minItems === 'number' && value.length < schema.minItems) {
      return addIssue(issues, path, `must contain at least ${String(schema.minItems)} items`);
    }
    if (typeof schema.maxItems === 'number' && value.length > schema.maxItems) {
      return addIssue(issues, path, `must contain at most ${String(schema.maxItems)} items`);
    }
    if (schema.uniqueItems === true) {
      for (let left = 0; left < value.length; left += 1) {
        for (let right = left + 1; right < value.length; right += 1) {
          if (Object.is(value[left], value[right])) {
            return addIssue(issues, path, 'must contain unique items');
          }
        }
      }
    }
    if (schema.items !== undefined) {
      for (const [index, item] of value.entries()) {
        if (
          !validateSchema(
            item,
            schema.items,
            `${path}[${String(index)}]`,
            issues,
            depth + 1
          )
        ) {
          return false;
        }
      }
    }
  }

  if (isObject(value) && allOf === undefined) {
    const shape = collectObjectShape(schema);
    if (shape !== undefined && !validateObject(value, shape, path, issues, depth)) {
      return false;
    }
  }

  return true;
}

function namedSchema(name: string): unknown {
  if (!isObject(openApi) || !isObject(openApi.components) || !isObject(openApi.components.schemas)) {
    return undefined;
  }
  return openApi.components.schemas[name];
}

export class WireValidationError extends Error {
  readonly issues: readonly string[];

  constructor(schemaName: string, issues: readonly string[]) {
    super(`invalid ${schemaName}: ${issues.join('; ')}`);
    this.name = 'WireValidationError';
    this.issues = issues;
  }
}

export function validateWireValue(schemaName: string, value: unknown): unknown {
  const issues: string[] = [];
  if (!validateSchema(value, namedSchema(schemaName), '$', issues, 0)) {
    throw new WireValidationError(schemaName, issues);
  }
  return value;
}

export function parseServerMessage(text: string): ServerMessage {
  const value = JSON.parse(text) as unknown;
  return validateWireValue('ServerMessage', value) as ServerMessage;
}

export function serializeClientMessage(message: ClientMessage): string {
  validateWireValue('ClientMessage', message);
  return JSON.stringify(message);
}

export function parseSettingsSnapshot(value: unknown): SettingsSnapshot {
  return validateWireValue('SettingsSnapshot', value) as SettingsSnapshot;
}

export function parseStateSnapshot(value: unknown): StateSnapshot {
  return validateWireValue('StateSnapshot', value) as StateSnapshot;
}
