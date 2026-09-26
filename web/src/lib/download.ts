import type { DownloadResult } from './actions';

function safeFilename(value: string, fallback: string): string {
  const filename = value.trim().replaceAll('\\', '/').split('/').at(-1)?.trim();
  return filename === undefined || filename.length === 0 ? fallback : filename;
}

export function downloadFilename(contentDisposition: string | null, fallback: string): string {
  if (contentDisposition === null) return fallback;
  const encoded = /filename\*=UTF-8''([^;]+)/iu.exec(contentDisposition)?.[1];
  if (encoded !== undefined) {
    try {
      return safeFilename(decodeURIComponent(encoded.trim()), fallback);
    } catch {
      return fallback;
    }
  }
  const quoted = /filename="([^"]+)"/iu.exec(contentDisposition)?.[1];
  return quoted === undefined ? fallback : safeFilename(quoted, fallback);
}

export function triggerDownload(result: DownloadResult, fallbackFilename: string): void {
  if (typeof document === 'undefined') return;
  const url = URL.createObjectURL(result.blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = downloadFilename(result.contentDisposition, fallbackFilename);
  anchor.hidden = true;
  document.body.append(anchor);
  anchor.click();
  anchor.remove();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}
