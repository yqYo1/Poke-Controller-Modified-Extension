import type { components } from './generated/api';

export type DecimalString = components['schemas']['DecimalString'];

const DECIMAL_PATTERN = /^(?:0|[1-9][0-9]*)$/u;

export function isDecimalString(value: unknown): value is DecimalString {
  return typeof value === 'string' && DECIMAL_PATTERN.test(value);
}

export function compareRevisions(left: DecimalString, right: DecimalString): number {
  if (!isDecimalString(left) || !isDecimalString(right)) {
    throw new TypeError('revision must be a canonical non-negative decimal string');
  }
  if (left.length !== right.length) {
    return left.length < right.length ? -1 : 1;
  }
  if (left === right) {
    return 0;
  }
  return left < right ? -1 : 1;
}

export function incrementRevision(revision: DecimalString): DecimalString {
  if (!isDecimalString(revision)) {
    throw new TypeError('revision must be a canonical non-negative decimal string');
  }

  let suffix = '';
  for (let index = revision.length - 1; index >= 0; index -= 1) {
    const codePoint = revision.charCodeAt(index);
    if (codePoint !== 57) {
      return `${revision.slice(0, index)}${String.fromCodePoint(codePoint + 1)}${suffix}`;
    }
    suffix = `0${suffix}`;
  }
  return `1${suffix}`;
}

export function minimumRevision(
  left: DecimalString,
  right: DecimalString
): DecimalString {
  return compareRevisions(left, right) <= 0 ? left : right;
}

export function maximumRevision(
  left: DecimalString,
  right: DecimalString
): DecimalString {
  return compareRevisions(left, right) >= 0 ? left : right;
}
