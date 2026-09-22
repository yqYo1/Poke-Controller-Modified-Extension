<script lang="ts">
  import { onMount } from 'svelte';

  import type { OperationResult, SerialControlRequest, SerialPort } from '../actions';
  import type { SettingsWriteValues } from '../api';
  import type { ApplicationRuntime, RuntimeView } from '../runtime';

  interface SerialActions {
    controlSerial(request: SerialControlRequest): Promise<OperationResult>;
    serialPorts(): Promise<readonly SerialPort[]>;
  }

  interface Props {
    actions: SerialActions;
    autoLoad?: boolean;
    runtime: ApplicationRuntime;
    view: RuntimeView;
  }

  let { actions, autoLoad = true, runtime, view }: Props = $props();
  let autoScroll = $state(true);
  let busy = $state<'connect' | 'disconnect' | 'ports' | 'settings' | null>(null);
  let error = $state<string | null>(null);
  let monitor: HTMLDivElement | undefined;
  let ports = $state<readonly SerialPort[]>([]);
  let portInput = $state('');
  let synchronizedPort = $state<string | null>(null);

  const effectivePort = $derived(view.settings?.values['serial.port'] ?? '');
  const baudRate = $derived(view.settings?.values['serial.baud_rate'] ?? 9600);
  const dataFormat = $derived(view.settings?.values['serial.data_format'] ?? 'default');
  const connected = $derived(view.state?.serial_connected ?? false);

  $effect(() => {
    if (effectivePort !== synchronizedPort) {
      synchronizedPort = effectivePort;
      portInput = effectivePort;
    }
  });

  $effect(() => {
    if (!autoScroll || monitor === undefined || view.serial.length === 0) return;
    const element = monitor;
    queueMicrotask(() => {
      element.scrollTop = element.scrollHeight;
    });
  });

  onMount(() => {
    try {
      autoScroll = localStorage.getItem('pokecon.serial.auto-scroll') !== 'false';
    } catch {
      // Client-only monitor preference is optional.
    }
    if (autoLoad) void refreshPorts();
  });

  $effect(() => {
    try {
      localStorage.setItem('pokecon.serial.auto-scroll', String(autoScroll));
    } catch {
      // Private browsing modes may make storage unavailable.
    }
  });

  function errorMessage(reason: unknown): string {
    return reason instanceof Error ? reason.message : 'Serial operation failed';
  }

  async function refreshPorts(): Promise<void> {
    busy = 'ports';
    error = null;
    try {
      ports = await actions.serialPorts();
    } catch (reason: unknown) {
      error = errorMessage(reason);
    } finally {
      busy = null;
    }
  }

  async function write(values: SettingsWriteValues): Promise<void> {
    busy = 'settings';
    error = null;
    try {
      await runtime.writeSettings(values);
    } catch (reason: unknown) {
      error = errorMessage(reason);
    } finally {
      busy = null;
    }
  }

  async function writePort(): Promise<void> {
    await write({ 'serial.port': portInput });
  }

  async function writeBaud(event: Event): Promise<void> {
    const value = Number((event.currentTarget as HTMLInputElement).value);
    if (!Number.isSafeInteger(value) || value <= 0) {
      error = 'Baud rate must be a positive integer.';
      return;
    }
    await write({ 'serial.baud_rate': value });
  }

  async function writeFormat(event: Event): Promise<void> {
    const format = (event.currentTarget as HTMLSelectElement).value as SettingsWriteValues['serial.data_format'];
    await write(
      format === '3ds'
        ? { 'serial.baud_rate': 115200, 'serial.data_format': format }
        : { 'serial.data_format': format }
    );
  }

  async function control(action: SerialControlRequest['action']): Promise<void> {
    busy = action;
    error = null;
    try {
      await actions.controlSerial({ action });
    } catch (reason: unknown) {
      error = errorMessage(reason);
    } finally {
      busy = null;
    }
  }
</script>

<div class="space-y-4">
  <div>
    <p class="text-xs font-semibold tracking-[0.12em] text-blue uppercase">Serial</p>
    <div class="mt-2 flex flex-wrap items-center justify-between gap-3">
      <h2 class="text-2xl font-semibold text-text">シリアルモニター</h2>
      <span class={`rounded-full px-3 py-1 text-xs ${connected ? 'bg-green/15 text-green' : 'bg-text/5 text-subtext1'}`}>
        {connected ? 'Connected' : 'Disconnected'}
      </span>
    </div>
  </div>

  {#if error !== null}
    <div class="rounded-lg border border-red/20 bg-red/10 px-3 py-2 text-sm text-red" role="alert">{error}</div>
  {/if}

  <fieldset class="grid gap-4 rounded-lg border border-text/10 bg-text/[0.025] p-4 sm:grid-cols-2" disabled={busy !== null}>
    <legend class="px-2 text-xs font-semibold tracking-[0.14em] text-subtext1 uppercase">Connection</legend>
    <label class="sm:col-span-2">
      <span class="text-xs text-subtext1">Port selector</span>
      <div class="mt-1 flex gap-2">
        <input class="min-w-0 flex-1 rounded-lg border border-text/10 bg-surface1 px-3 py-2 text-sm text-text" list="serial-port-options" bind:value={portInput} onchange={() => void writePort()} placeholder="/dev/ttyACM0 or COM3" />
        <datalist id="serial-port-options">
          {#each ports as port (port.selector)}
            <option value={port.selector}>{port.label}{port.available ? '' : ' (unavailable)'}</option>
          {/each}
        </datalist>
        <button type="button" class="rounded-lg bg-text/5 px-3 py-2 text-sm text-subtext1 hover:bg-text/10" onclick={() => void refreshPorts()}>Refresh</button>
      </div>
      {#if connected && view.state?.serial_port !== null}
        <span class="mt-1 block text-xs text-subtext0">Active: {view.state?.serial_port}</span>
      {/if}
    </label>

    <label>
      <span class="text-xs text-subtext1">Baud rate</span>
      <input class="mt-1 w-full rounded-lg border border-text/10 bg-surface1 px-3 py-2 text-sm text-text" type="number" min="1" step="1" list="serial-baud-options" value={baudRate} onchange={(event) => void writeBaud(event)} />
      <datalist id="serial-baud-options"><option value="4800"></option><option value="9600"></option><option value="115200"></option></datalist>
    </label>

    <label>
      <span class="text-xs text-subtext1">Data format</span>
      <select class="mt-1 w-full rounded-lg border border-text/10 bg-surface1 px-3 py-2 text-sm text-text" value={dataFormat} onchange={(event) => void writeFormat(event)}>
        <option value="default">Default</option>
        <option value="qingpi">Qingpi</option>
        <option value="3ds">3DS Controller</option>
      </select>
    </label>

    <div class="flex gap-2 sm:col-span-2">
      <button type="button" class="rounded-lg bg-blue/15 px-4 py-2 text-sm font-medium text-blue disabled:opacity-40" disabled={effectivePort.length === 0} onclick={() => void control('connect')}>Connect</button>
      <button type="button" class="rounded-lg bg-red/10 px-4 py-2 text-sm font-medium text-red disabled:opacity-40" disabled={!connected} onclick={() => void control('disconnect')}>Disconnect</button>
    </div>
  </fieldset>

  <section class="overflow-hidden rounded-lg border border-text/10 bg-crust/25" aria-label="Serial data monitor">
    <div class="flex items-center justify-between border-b border-text/10 px-3 py-2">
      <h3 class="text-xs font-semibold tracking-[0.14em] text-subtext1 uppercase">Raw receive data</h3>
      <div class="flex gap-1">
        <button type="button" class={`rounded-md px-2 py-1 text-[11px] ${autoScroll ? 'bg-blue/15 text-blue' : 'bg-text/5 text-subtext1'}`} aria-pressed={autoScroll} onclick={() => (autoScroll = !autoScroll)}>Auto</button>
        <button type="button" class="rounded-md bg-text/5 px-2 py-1 text-[11px] text-subtext1" onclick={() => runtime.clearSerial()}>Clear</button>
      </div>
    </div>
    <div bind:this={monitor} class="h-64 overflow-auto p-3 font-mono text-xs leading-5 text-subtext1" aria-live="polite">
      {#if view.serial.length === 0}
        <p class="text-subtext0">No serial data</p>
      {:else}
        {#each view.serial as line (line.id)}
          <div><span class="mr-2 select-none text-subtext0">{String(line.byteLength)} B</span>{line.text}</div>
        {/each}
      {/if}
    </div>
  </section>
</div>
