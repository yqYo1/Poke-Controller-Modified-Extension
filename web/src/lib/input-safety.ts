export const INPUT_NEUTRALIZED_EVENT = 'pokecon:input-neutralized';

type InputNeutralizedListener = () => void;

export function onInputNeutralized(listener: InputNeutralizedListener): () => void {
  if (typeof window === 'undefined') {
    return () => undefined;
  }
  const handler = (): void => listener();
  window.addEventListener(INPUT_NEUTRALIZED_EVENT, handler);
  return () => {
    window.removeEventListener(INPUT_NEUTRALIZED_EVENT, handler);
  };
}

export function dispatchInputNeutralized(): void {
  if (typeof window !== 'undefined') {
    window.dispatchEvent(new Event(INPUT_NEUTRALIZED_EVENT));
  }
}

export function isKeyboardOwnedTarget(target: EventTarget | null): boolean {
  if (typeof Element === 'undefined' || !(target instanceof Element)) return false;
  if (
    target instanceof HTMLElement &&
    (target.isContentEditable ||
      target.getAttribute('contenteditable') === '' ||
      target.getAttribute('contenteditable') === 'true' ||
      ['BUTTON', 'INPUT', 'SELECT', 'TEXTAREA'].includes(target.tagName))
  ) {
    return true;
  }
  return (
    target.closest(
      'a[href], [role="menu"], [role="menuitem"], [role="dialog"], [aria-modal="true"], [data-keyboard-ignore]'
    ) !== null
  );
}
