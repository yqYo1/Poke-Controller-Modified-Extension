import { describe, expect, it } from 'vitest';

import {
  compareRevisions,
  incrementRevision,
  isDecimalString,
  maximumRevision,
  minimumRevision
} from './revision';

describe('decimal revisions', () => {
  it('rejects non-canonical values', () => {
    expect(isDecimalString('0')).toBe(true);
    expect(isDecimalString('18446744073709551616')).toBe(true);
    expect(isDecimalString('01')).toBe(false);
    expect(isDecimalString('-1')).toBe(false);
    expect(isDecimalString(1)).toBe(false);
  });

  it('compares values without Number precision loss', () => {
    const smaller = '90071992547409929999999999999999999999';
    const larger = '100000000000000000000000000000000000000';
    expect(compareRevisions(smaller, larger)).toBeLessThan(0);
    expect(compareRevisions(larger, smaller)).toBeGreaterThan(0);
    expect(minimumRevision(smaller, larger)).toBe(smaller);
    expect(maximumRevision(smaller, larger)).toBe(larger);
  });

  it('increments an arbitrarily long value', () => {
    expect(incrementRevision('0')).toBe('1');
    expect(incrementRevision('899999999999999999999')).toBe('900000000000000000000');
    expect(incrementRevision('999999999999999999999')).toBe('1000000000000000000000');
  });
});
