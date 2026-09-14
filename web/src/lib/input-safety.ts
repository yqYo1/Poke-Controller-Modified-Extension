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
