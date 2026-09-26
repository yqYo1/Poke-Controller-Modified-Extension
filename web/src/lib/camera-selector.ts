export type CameraSelector = number | string;

/**
 * Returns a stable identity for selectors that may be represented by either
 * the native numeric index or its Linux device path.
 */
export function cameraSelectorIdentity(selector: CameraSelector): string {
  if (typeof selector === 'number' && Number.isSafeInteger(selector) && selector >= 0) {
    return `index:${String(selector)}`;
  }
  if (typeof selector === 'string') {
    const value = selector;
    const match = /^\/dev\/video(0|[1-9][0-9]*)$/.exec(value);
    if (match !== null) {
      const index = Number(match[1]);
      if (Number.isSafeInteger(index)) {
        return `index:${String(index)}`;
      }
    }
    return `name:${value}`;
  }
  return `name:${String(selector)}`;
}

export function sameCameraSelector(left: CameraSelector, right: CameraSelector): boolean {
  return cameraSelectorIdentity(left) === cameraSelectorIdentity(right);
}

export function cameraSelectorKey(selector: CameraSelector): string {
  return cameraSelectorIdentity(selector);
}
