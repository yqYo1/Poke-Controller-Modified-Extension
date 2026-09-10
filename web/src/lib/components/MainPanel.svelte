<script lang="ts">
  import type {
    DownloadResult,
    OperationResult,
    SavedScreenshot,
    ScreenshotRequest,
    ScriptUiAction,
    ScriptUiActionResult
  } from '../actions';
  import type { SettingsWriteValues } from '../api';
  import type { components } from '../api/openapi';
  import { chooseNativeSavePath, isDesktopShell } from '../desktop';
  import { triggerDownload } from '../download';
  import type { ApplicationRuntime, RuntimeView } from '../runtime';
  import CameraViewport from './CameraViewport.svelte';

  type DownloadRequest = Extract<ScreenshotRequest, { destination: 'download' }>;
  type SavedRequest = Exclude<ScreenshotRequest, DownloadRequest>;
  type ImageFormat = components['schemas']['ImageFormat'];
  type NormalizedRegion = components['schemas']['NormalizedRegion'];
  type TouchscreenArea = NonNullable<SettingsWriteValues['input.touchscreen_area']>;

  interface MainPanelActions {
    downloadScreenshot(request: DownloadRequest): Promise<DownloadResult>;
    retryCamera(): Promise<OperationResult>;
    saveScreenshot(request: SavedRequest): Promise<SavedScreenshot>;
    scriptUiAction(request: ScriptUiAction): Promise<ScriptUiActionResult>;
  }

  interface Props {
    actions: MainPanelActions;
    onnavigate?: (target: 'camera' | 'commands' | 'notifications') => void;
    runtime: ApplicationRuntime;
    view: RuntimeView;
  }

  let { actions, onnavigate, runtime, view }: Props = $props();
  let busy = $state<'capture' | 'download' | 'retry' | null>(null);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  const desktopMode = isDesktopShell();

  const values = $derived(view.settings?.values);
  const format = $derived(values?.['camera.screenshot_format'] ?? 'png');

  function errorMessage(reason: unknown): string {
    return reason instanceof Error ? reason.message : 'Camera operation failed';
  }

  function timestampName(imageFormat: ImageFormat): string {
    const date = new Date();
    const part = (value: number): string => String(value).padStart(2, '0');
    return `capture_${String(date.getFullYear())}${part(date.getMonth() + 1)}${part(date.getDate())}_${part(date.getHours())}${part(date.getMinutes())}${part(date.getSeconds())}.${imageFormat === 'jpeg' ? 'jpg' : 'png'}`;
  }

  async function saveCapture(region: NormalizedRegion | null): Promise<void> {
    busy = 'capture';
    error = null;
    notice = null;
    try {
      const saved = await actions.saveScreenshot({
        destination: 'captures',
        format,
        region
      });
      notice = `Saved: ${saved.display_path}`;
    } catch (reason: unknown) {
      error = errorMessage(reason);
    } finally {
      busy = null;
    }
  }

  async function downloadCapture(region: NormalizedRegion | null): Promise<void> {
    busy = 'download';
    error = null;
    notice = null;
    const filename = timestampName(format);
    try {
      if (desktopMode) {
        const path = await chooseNativeSavePath(filename, format);
        if (path === null) {
          notice = 'Save cancelled.';
          return;
        }
        const saved = await actions.saveScreenshot({
          destination: 'path',
          format,
          overwrite: true,
          path,
          region
        });
        notice = `Saved: ${saved.display_path}`;
        return;
      }
      const result = await actions.downloadScreenshot({
        destination: 'download',
        filename,
        format,
        region
      });
      triggerDownload(result, filename);
      notice = `Download prepared: ${filename}`;
    } catch (reason: unknown) {
      error = errorMessage(reason);
    } finally {
      busy = null;
    }
  }

  async function retry(): Promise<void> {
    busy = 'retry';
    error = null;
    notice = null;
    try {
      await actions.retryCamera();
      notice = 'Camera retry completed.';
    } catch (reason: unknown) {
      error = errorMessage(reason);
    } finally {
      busy = null;
    }
  }

  function updateTouchArea(area: TouchscreenArea): void {
    void runtime.writeSettings({ 'input.touchscreen_area': area }).catch(() => undefined);
  }

  function captureRegion(region: NormalizedRegion): void {
    void saveCapture(region);
  }

  function downloadRegion(region: NormalizedRegion): void {
    void downloadCapture(region);
  }

  function focusController(): void {
    const panel = document.getElementById('software-controller');
    if (panel === null) return;
    panel.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    panel.querySelector<HTMLElement>('button')?.focus({ preventScroll: true });
  }
</script>

<section
  aria-label="Main Panel"
  class="min-w-0 rounded-2xl border border-white/10 bg-ink-900/80 p-4 shadow-2xl shadow-black/20"
>
  <div class="flex flex-wrap items-center gap-2" role="toolbar" aria-label="Quick actions">
    <button
      type="button"
      class="rounded-lg bg-lime-300/15 px-4 py-2 text-sm font-medium text-lime-200 disabled:opacity-40"
      disabled={busy !== null}
      title="Open the Commands tab to start the selected command"
      onclick={() => onnavigate?.('commands')}
    >Start</button>
    <button
      type="button"
      class="rounded-lg bg-white/5 px-4 py-2 text-sm text-slate-200 hover:bg-white/10"
      title="Focus the Software-Controller in the right column"
      onclick={focusController}
    >Controller</button>
    <button
      type="button"
      class="rounded-lg bg-white/5 px-4 py-2 text-sm text-slate-200 hover:bg-white/10"
      title="Clear Output #1 and Output #2"
      onclick={() => runtime.clearOutputs()}
    >Clear Outputs</button>
    <button
      type="button"
      class="rounded-lg bg-cyan-300/15 px-4 py-2 text-sm font-medium text-cyan-200 disabled:opacity-40"
      disabled={busy !== null}
      title="Save the current frame to Captures"
      onclick={() => void saveCapture(null)}
    >Capture</button>
    <button
      type="button"
      class="rounded-lg bg-white/5 px-4 py-2 text-sm text-slate-200 hover:bg-white/10"
      title="Open Camera settings (no folder-open API in Web mode)"
      onclick={() => onnavigate?.('camera')}
    >Capture folder</button>
    <button
      type="button"
      class="rounded-lg bg-white/5 px-4 py-2 text-sm text-slate-200 hover:bg-white/10"
      title="Open notification settings (Discord image send has no Web API)"
      onclick={() => onnavigate?.('notifications')}
    >Discord</button>
  </div>

  <div class="mt-3 flex flex-wrap items-center gap-2 text-xs">
    <span
      class={`rounded-full px-3 py-1 ${view.state?.camera_opened ? 'bg-lime-300/15 text-lime-300' : 'bg-red-400/10 text-red-200'}`}
    >
      {view.state?.camera_opened ? 'Camera open' : 'Camera closed'}
    </span>
    <span class="rounded-full bg-white/5 px-3 py-1 text-slate-400">{view.media.mode}</span>
  </div>

  <div id="main-camera-preview" class="mt-3 min-w-0">
    <CameraViewport
      {actions}
      fps={values?.['ui.fps'] ?? 30}
      guideVisible={values?.['ui.camera.guide_visible'] ?? false}
      leftStickEnabled={values?.['input.left_stick_mouse_enabled'] ?? false}
      liveViewEnabled={values?.['ui.camera.live_view_enabled'] ?? true}
      oncapture={captureRegion}
      ondownload={downloadRegion}
      ontoucharea={updateTouchArea}
      pixelValuesVisible={values?.['ui.camera.pixel_values_visible'] ?? false}
      rightStickEnabled={values?.['input.right_stick_mouse_enabled'] ?? false}
      {runtime}
      {view}
    />
  </div>

  {#if error !== null || view.media.lastError !== null}
    <div
      class="mt-3 flex flex-wrap items-center justify-between gap-3 rounded-lg border border-red-400/20 bg-red-400/10 px-3 py-2 text-sm text-red-200"
      role="alert"
    >
      <span>{error ?? view.media.lastError}</span>
      <div class="flex gap-2">
        <button
          type="button"
          class="rounded-md bg-white/10 px-2 py-1 text-xs"
          disabled={busy !== null}
          onclick={() => void retry()}
        >Retry camera</button>
        {#if view.media.mode !== 'webrtc'}
          <button
            type="button"
            class="rounded-md bg-white/10 px-2 py-1 text-xs"
            onclick={() => runtime.reconnectWebRtc()}
          >Retry WebRTC</button>
        {/if}
      </div>
    </div>
  {:else if notice !== null}
    <div
      class="mt-3 rounded-lg border border-lime-300/20 bg-lime-300/10 px-3 py-2 text-sm text-lime-200"
      role="status"
    >{notice}</div>
  {/if}
</section>
