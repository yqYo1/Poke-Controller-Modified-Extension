interface TauriInternals {
  invoke<T>(command: string, arguments_?: Record<string, unknown>): Promise<T>;
}

export type NativeSaveExtension = 'bat' | 'jpeg' | 'jpg' | 'png';

function tauriInternals(): TauriInternals | null {
  if (typeof window === 'undefined') return null;
  const candidate: unknown = Reflect.get(window, '__TAURI_INTERNALS__');
  if (
    typeof candidate !== 'object' ||
    candidate === null ||
    typeof Reflect.get(candidate, 'invoke') !== 'function'
  ) {
    return null;
  }
  return candidate as TauriInternals;
}

export function isDesktopShell(): boolean {
  return tauriInternals() !== null;
}

export async function chooseNativeSavePath(
  suggestedName: string,
  extension: NativeSaveExtension
): Promise<string | null> {
  const internals = tauriInternals();
  if (internals === null) throw new Error('native save dialog is unavailable in Web mode');
  return internals.invoke<string | null>('choose_save_path', {
    extension,
    suggestedName
  });
}

export async function openNativeConfigDirectory(): Promise<void> {
  const internals = tauriInternals();
  if (internals === null) throw new Error('native config directory access is unavailable in Web mode');
  await internals.invoke<unknown>('open_config_directory');
}
