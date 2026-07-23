<script lang="ts">
  import { onMount } from 'svelte';

  import type {
    CameraDevice,
    DownloadResult,
    OperationResult,
    SavedScreenshot,
    ScriptUiAction,
    ScriptUiActionResult,
    ScreenshotRequest
  } from '../actions';
  import type { SettingsWriteValues } from '../api';
  import { chooseNativeSavePath, isDesktopShell } from '../desktop';
  import { triggerDownload } from '../download';
  import type { components } from '../generated/api';
  import type { ApplicationRuntime, RuntimeView } from '../runtime';
  import CameraViewport from './CameraViewport.svelte';

  type DownloadRequest = Extract<ScreenshotRequest, { destination: 'download' }>;
  type SavedRequest = Exclude<ScreenshotRequest, DownloadRequest>;
  type ImageFormat = components['schemas']['ImageFormat'];
  type NormalizedRegion = components['schemas']['NormalizedRegion'];
  type TouchscreenArea = NonNullable<SettingsWriteValues['input.touchscreen_area']>;

  interface CameraActions {
    cameras(): Promise<readonly CameraDevice[]>;
    downloadScreenshot(request: DownloadRequest): Promise<DownloadResult>;
    retryCamera(): Promise<OperationResult>;
    saveScreenshot(request: SavedRequest): Promise<SavedScreenshot>;
    scriptUiAction(request: ScriptUiAction): Promise<ScriptUiActionResult>;
  }

  interface Props {
    actions: CameraActions;
    autoLoad?: boolean;
    runtime: ApplicationRuntime;
    view: RuntimeView;
  }

  interface DeviceOption extends CameraDevice {
    readonly key: string;
  }

  let { actions, autoLoad = true, runtime, view }: Props = $props();
  let busy = $state<'devices' | 'download' | 'retry' | 'save' | 'settings' | null>(null);
  let devices = $state<readonly CameraDevice[]>([]);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  const desktopMode = isDesktopShell();

  const values = $derived(view.settings?.values);
  const currentDevice = $derived(values?.['camera.device'] ?? 0);
  const deviceOptions = $derived.by<readonly DeviceOption[]>(() => {
    const options = [...devices];
    if (!options.some((device) => sameSelector(device.selector, currentDevice))) {
      options.unshift({
        available: false,
        label: `${String(currentDevice)} (configured, unavailable)`,
        selector: currentDevice
      });
    }
    return options.map((device) => ({ ...device, key: selectorKey(device.selector) }));
  });

  onMount(() => {
    if (autoLoad) void refreshDevices();
  });

  function sameSelector(left: number | string, right: number | string): boolean {
    return typeof left === typeof right && left === right;
  }

  function selectorKey(selector: number | string): string {
    return `${typeof selector}:${String(selector)}`;
  }

  function errorMessage(reason: unknown): string {
    return reason instanceof Error ? reason.message : 'Camera operation failed';
  }

  async function refreshDevices(): Promise<void> {
    busy = 'devices';
    error = null;
    try {
      devices = await actions.cameras();
    } catch (reason: unknown) {
      error = errorMessage(reason);
    } finally {
      busy = null;
    }
  }

  async function write(settings: SettingsWriteValues): Promise<void> {
    busy = 'settings';
    error = null;
    notice = null;
    try {
      await runtime.writeSettings(settings);
    } catch (reason: unknown) {
      error = errorMessage(reason);
    } finally {
      busy = null;
    }
  }

  async function changeDevice(event: Event): Promise<void> {
    const key = (event.currentTarget as HTMLSelectElement).value;
    const option = deviceOptions.find((candidate) => candidate.key === key);
    if (option !== undefined) await write({ 'camera.device': option.selector });
  }

  async function changePositiveInteger(
    event: Event,
    setting: 'camera.capture_fps' | 'ui.fps'
  ): Promise<void> {
    const value = Number((event.currentTarget as HTMLInputElement | HTMLSelectElement).value);
    if (!Number.isSafeInteger(value) || value <= 0) {
      error = 'FPS must be a positive integer.';
      return;
    }
    await write({ [setting]: value });
  }

  async function changeTextSetting(
    event: Event,
    setting: 'camera.capture_resolution' | 'camera.flip_mode' | 'camera.screenshot_format'
  ): Promise<void> {
    const value = (event.currentTarget as HTMLSelectElement).value;
    if (setting === 'camera.capture_resolution') {
      await write({ 'camera.capture_resolution': value as SettingsWriteValues['camera.capture_resolution'] });
    } else if (setting === 'camera.flip_mode') {
      await write({ 'camera.flip_mode': value as SettingsWriteValues['camera.flip_mode'] });
    } else {
      await write({ 'camera.screenshot_format': value as SettingsWriteValues['camera.screenshot_format'] });
    }
  }

  async function changeToggle(
    event: Event,
    setting:
      | 'ui.camera.guide_visible'
      | 'ui.camera.live_view_enabled'
      | 'ui.camera.pixel_values_visible'
  ): Promise<void> {
    await write({ [setting]: (event.currentTarget as HTMLInputElement).checked });
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

  function timestampName(format: ImageFormat): string {
    const date = new Date();
    const part = (value: number): string => String(value).padStart(2, '0');
    return `capture_${String(date.getFullYear())}${part(date.getMonth() + 1)}${part(date.getDate())}_${part(date.getHours())}${part(date.getMinutes())}${part(date.getSeconds())}.${format === 'jpeg' ? 'jpg' : 'png'}`;
  }

  async function saveCapture(region: NormalizedRegion | null): Promise<void> {
    busy = 'save';
    error = null;
    notice = null;
    try {
      const saved = await actions.saveScreenshot({
        destination: 'captures',
        format: values?.['camera.screenshot_format'] ?? 'png',
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
    const format = values?.['camera.screenshot_format'] ?? 'png';
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

  function updateTouchArea(area: TouchscreenArea): void {
    void write({ 'input.touchscreen_area': area });
  }

  function captureRegion(region: NormalizedRegion): void {
    void saveCapture(region);
  }

  function downloadRegion(region: NormalizedRegion): void {
    void downloadCapture(region);
  }
</script>

<div class="space-y-4">
  <div>
    <p class="text-xs font-semibold tracking-[0.2em] text-lime-300 uppercase">Camera</p>
    <div class="mt-2 flex flex-wrap items-center justify-between gap-3">
      <h2 class="text-2xl font-semibold text-white">ライブキャプチャ</h2>
      <div class="flex items-center gap-2 text-xs">
        <span class={`rounded-full px-3 py-1 ${view.state?.camera_opened ? 'bg-lime-300/15 text-lime-300' : 'bg-red-400/10 text-red-200'}`}>
          {view.state?.camera_opened ? 'Camera open' : 'Camera closed'}
        </span>
        <span class="rounded-full bg-white/5 px-3 py-1 text-slate-400">{view.media.mode}</span>
      </div>
    </div>
  </div>

  {#if error !== null || view.media.lastError !== null}
    <div class="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-red-400/20 bg-red-400/10 px-3 py-2 text-sm text-red-200" role="alert">
      <span>{error ?? view.media.lastError}</span>
      <div class="flex gap-2">
        <button type="button" class="rounded-md bg-white/10 px-2 py-1 text-xs" disabled={busy !== null} onclick={() => void retry()}>Retry camera</button>
        {#if view.media.mode !== 'webrtc'}
          <button type="button" class="rounded-md bg-white/10 px-2 py-1 text-xs" onclick={() => runtime.reconnectWebRtc()}>Retry WebRTC</button>
        {/if}
      </div>
    </div>
  {:else if notice !== null}
    <div class="rounded-lg border border-lime-300/20 bg-lime-300/10 px-3 py-2 text-sm text-lime-200" role="status">{notice}</div>
  {/if}

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

  <fieldset class="grid gap-4 rounded-xl border border-white/10 bg-white/[0.025] p-4 sm:grid-cols-2 xl:grid-cols-3" disabled={busy !== null}>
    <legend class="px-2 text-xs font-semibold tracking-[0.14em] text-slate-400 uppercase">Capture settings</legend>
    <label class="sm:col-span-2 xl:col-span-3">
      <span class="text-xs text-slate-400">Camera device</span>
      <div class="mt-1 flex gap-2">
        <select class="min-w-0 flex-1 rounded-lg border border-white/10 bg-ink-800 px-3 py-2 text-sm text-white" value={selectorKey(currentDevice)} onchange={(event) => void changeDevice(event)}>
          {#each deviceOptions as device (device.key)}
            <option value={device.key}>{device.label}{device.available ? '' : ' (unavailable)'}</option>
          {/each}
        </select>
        <button type="button" class="rounded-lg bg-white/5 px-3 py-2 text-sm text-slate-300 hover:bg-white/10" onclick={() => void refreshDevices()}>Refresh</button>
      </div>
    </label>

    <label>
      <span class="text-xs text-slate-400">UI FPS</span>
      <select class="mt-1 w-full rounded-lg border border-white/10 bg-ink-800 px-3 py-2 text-sm text-white" value={values?.['ui.fps'] ?? 30} onchange={(event) => void changePositiveInteger(event, 'ui.fps')}>
        {#each values?.['ui.fps_options'] ?? [5, 15, 30, 60] as option (option)}
          <option value={option}>{String(option)}</option>
        {/each}
      </select>
    </label>

    <label>
      <span class="text-xs text-slate-400">Capture FPS</span>
      <input class="mt-1 w-full rounded-lg border border-white/10 bg-ink-800 px-3 py-2 text-sm text-white" type="number" min="1" step="1" value={values?.['camera.capture_fps'] ?? 60} onchange={(event) => void changePositiveInteger(event, 'camera.capture_fps')} />
    </label>

    <label>
      <span class="text-xs text-slate-400">Resolution</span>
      <select class="mt-1 w-full rounded-lg border border-white/10 bg-ink-800 px-3 py-2 text-sm text-white" value={values?.['camera.capture_resolution'] ?? '1280x720'} onchange={(event) => void changeTextSetting(event, 'camera.capture_resolution')}>
        <option value="640x360">640 × 360</option><option value="1280x720">1280 × 720</option><option value="1920x1080">1920 × 1080</option>
      </select>
    </label>

    <label>
      <span class="text-xs text-slate-400">Flip</span>
      <select class="mt-1 w-full rounded-lg border border-white/10 bg-ink-800 px-3 py-2 text-sm text-white" value={values?.['camera.flip_mode'] ?? 'none'} onchange={(event) => void changeTextSetting(event, 'camera.flip_mode')}>
        <option value="none">None</option><option value="vertical">Vertical</option><option value="horizontal">Horizontal</option><option value="both">Both</option>
      </select>
    </label>

    <label>
      <span class="text-xs text-slate-400">Screenshot format</span>
      <select class="mt-1 w-full rounded-lg border border-white/10 bg-ink-800 px-3 py-2 text-sm text-white" value={values?.['camera.screenshot_format'] ?? 'png'} onchange={(event) => void changeTextSetting(event, 'camera.screenshot_format')}>
        <option value="png">PNG</option><option value="jpeg">JPEG</option>
      </select>
    </label>

    <div class="flex flex-wrap items-end gap-4">
      <label class="flex items-center gap-2 text-sm text-slate-300"><input type="checkbox" checked={values?.['ui.camera.live_view_enabled'] ?? true} onchange={(event) => void changeToggle(event, 'ui.camera.live_view_enabled')} />Live</label>
      <label class="flex items-center gap-2 text-sm text-slate-300"><input type="checkbox" checked={values?.['ui.camera.pixel_values_visible'] ?? false} onchange={(event) => void changeToggle(event, 'ui.camera.pixel_values_visible')} />Pixels</label>
      <label class="flex items-center gap-2 text-sm text-slate-300"><input type="checkbox" checked={values?.['ui.camera.guide_visible'] ?? false} onchange={(event) => void changeToggle(event, 'ui.camera.guide_visible')} />Guide</label>
    </div>

    <div class="flex flex-wrap gap-2 sm:col-span-2 xl:col-span-3">
      <button type="button" class="rounded-lg bg-lime-300/15 px-4 py-2 text-sm font-medium text-lime-200" onclick={() => void saveCapture(null)}>Save to Captures</button>
      <button type="button" class="rounded-lg bg-cyan-300/15 px-4 py-2 text-sm font-medium text-cyan-200" onclick={() => void downloadCapture(null)}>{desktopMode ? 'Save as…' : 'Download'}</button>
      <button type="button" class="rounded-lg bg-white/5 px-4 py-2 text-sm text-slate-300" onclick={() => void retry()}>Retry camera</button>
    </div>
  </fieldset>
</div>
