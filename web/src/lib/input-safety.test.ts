import { describe, expect, it } from 'vitest';

import { isKeyboardOwnedTarget } from './input-safety';

function element(html: string): Element {
  const template = document.createElement('template');
  template.innerHTML = html;
  const first = template.content.firstElementChild;
  if (first === null) throw new Error('fixture did not create an element');
  return first;
}

describe('isKeyboardOwnedTarget', () => {
  it('owns keys for editable and interactive controls', () => {
    expect(isKeyboardOwnedTarget(element('<button>run</button>'))).toBe(true);
    expect(isKeyboardOwnedTarget(element('<input />'))).toBe(true);
    expect(isKeyboardOwnedTarget(element('<select></select>'))).toBe(true);
    expect(isKeyboardOwnedTarget(element('<textarea></textarea>'))).toBe(true);
    expect(isKeyboardOwnedTarget(element('<div contenteditable="true"></div>'))).toBe(true);
  });

  it('owns keys for menus, dialogs and explicit opt-out elements', () => {
    expect(isKeyboardOwnedTarget(element('<div role="menuitem"><span>item</span></div>'))).toBe(
      true
    );
    expect(isKeyboardOwnedTarget(element('<div role="menu"></div>'))).toBe(true);
    expect(isKeyboardOwnedTarget(element('<div role="dialog"></div>'))).toBe(true);
    expect(isKeyboardOwnedTarget(element('<div aria-modal="true"></div>'))).toBe(true);
    expect(isKeyboardOwnedTarget(element('<div data-keyboard-ignore></div>'))).toBe(true);
    expect(isKeyboardOwnedTarget(element('<a href="/help">help</a>'))).toBe(true);
  });

  it('leaves keys to the game input for plain content', () => {
    expect(isKeyboardOwnedTarget(element('<div>camera</div>'))).toBe(false);
    expect(isKeyboardOwnedTarget(null)).toBe(false);
  });
});
