import { describe, expect, it } from 'vitest';

import {
  cameraSelectorIdentity,
  cameraSelectorKey,
  sameCameraSelector
} from './camera-selector';

describe('camera selector identity', () => {
  it('treats a Linux device path and its enumerated index as the same camera', () => {
    expect(sameCameraSelector('/dev/video10', 10)).toBe(true);
    expect(cameraSelectorIdentity('/dev/video010')).toBe('name:/dev/video010');
  });

  it('does not collapse unrelated native names', () => {
    expect(sameCameraSelector('/dev/video10', '/dev/video11')).toBe(false);
    expect(sameCameraSelector('camera-10', 10)).toBe(false);
  });

  it('keeps selector option keys type-sensitive', () => {
    expect(cameraSelectorKey(10)).toBe('number:10');
    expect(cameraSelectorKey('/dev/video10')).toBe('string:/dev/video10');
  });
});
