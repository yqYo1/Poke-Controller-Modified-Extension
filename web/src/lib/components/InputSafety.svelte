<script lang="ts">
  import { onMount } from 'svelte';

  import type { ApplicationRuntime, RuntimeView } from '../runtime';

  interface Props {
    runtime: ApplicationRuntime;
    view: RuntimeView;
  }

  let { runtime, view }: Props = $props();
  const pressedKeys: string[] = [];
  const keyboardEnabled = $derived(view.settings?.values['input.keyboard_enabled'] ?? true);

  function interactiveTarget(target: EventTarget | null): boolean {
    return (
      target instanceof HTMLElement &&
      (target.isContentEditable ||
        ['BUTTON', 'INPUT', 'SELECT', 'TEXTAREA'].includes(target.tagName))
    );
  }

  function keydown(event: KeyboardEvent): void {
    if (
      !keyboardEnabled ||
      event.repeat ||
      event.code.length === 0 ||
      event.code === 'Tab' ||
      event.metaKey ||
      event.ctrlKey ||
      event.altKey ||
      interactiveTarget(event.target) ||
      pressedKeys.includes(event.code)
    ) {
      return;
    }
    event.preventDefault();
    pressedKeys.push(event.code);
    runtime.setKeyboardKey(event.code, true);
  }

  function keyup(event: KeyboardEvent): void {
    const index = pressedKeys.indexOf(event.code);
    if (index < 0) return;
    event.preventDefault();
    pressedKeys.splice(index, 1);
    runtime.setKeyboardKey(event.code, false);
  }

  function releaseKeyboard(): void {
    for (const code of pressedKeys.splice(0)) runtime.setKeyboardKey(code, false);
  }

  function neutralize(): void {
    pressedKeys.splice(0);
    runtime.neutralizeInput();
  }

  $effect(() => {
    if (!keyboardEnabled) releaseKeyboard();
  });

  onMount(() => {
    const visibility = (): void => {
      if (document.visibilityState === 'hidden') neutralize();
    };
    window.addEventListener('blur', neutralize);
    window.addEventListener('keydown', keydown);
    window.addEventListener('keyup', keyup);
    document.addEventListener('visibilitychange', visibility);
    return () => {
      window.removeEventListener('blur', neutralize);
      window.removeEventListener('keydown', keydown);
      window.removeEventListener('keyup', keyup);
      document.removeEventListener('visibilitychange', visibility);
      neutralize();
    };
  });
</script>

<span hidden aria-hidden="true"></span>
