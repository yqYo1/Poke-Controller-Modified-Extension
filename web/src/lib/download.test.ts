import { describe, expect, it } from 'vitest';

import { downloadFilename } from './download';

describe('downloadFilename', () => {
  it('prefers encoded content-disposition names and strips path components', () => {
    expect(
      downloadFilename("attachment; filename*=UTF-8''captures%2F%E3%83%86%E3%82%B9%E3%83%88.png", 'fallback.png')
    ).toBe('テスト.png');
    expect(downloadFilename('attachment; filename="C:\\temp\\capture.jpg"', 'fallback.jpg')).toBe(
      'capture.jpg'
    );
  });

  it('uses the fallback for absent or malformed encoded names', () => {
    expect(downloadFilename(null, 'capture.png')).toBe('capture.png');
    expect(downloadFilename("attachment; filename*=UTF-8''%ZZ", 'capture.png')).toBe(
      'capture.png'
    );
  });
});
