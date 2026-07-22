import { ApiRequestError, api, responseError, type ApiClient } from './api';
import type { components } from './generated/api';

export type CameraDevice = components['schemas']['CameraDevice'];
export type CommandControlRequest = components['schemas']['CommandControlRequest'];
export type DynamicConfigControlRequest = components['schemas']['DynamicConfigControlRequest'];
export type DynamicConfigResult = components['schemas']['DynamicConfigResult'];
export type GenerateLauncherRequest = components['schemas']['GenerateLauncherRequest'];
export type GenerateLauncherResult = components['schemas']['GenerateLauncherResult'];
export type NotificationTestRequest = components['schemas']['NotificationTestRequest'];
export type NotificationTestResult = components['schemas']['NotificationTestResult'];
export type OperationResult = components['schemas']['OperationResult'];
export type SavedScreenshot = components['schemas']['SavedScreenshot'];
export type ScreenshotRequest = components['schemas']['ScreenshotRequest'];
export type ScriptUiAction = components['schemas']['ScriptUiAction'];
export type ScriptUiActionResult = components['schemas']['ScriptUiActionResult'];
export type SerialControlRequest = components['schemas']['SerialControlRequest'];
export type SerialPort = components['schemas']['SerialPort'];
export type UpdateCheckResult = components['schemas']['UpdateCheckResult'];

type DownloadScreenshotRequest = Extract<ScreenshotRequest, { destination: 'download' }>;
type SavedScreenshotRequest = Exclude<ScreenshotRequest, DownloadScreenshotRequest>;
type DownloadLauncherRequest = GenerateLauncherRequest & {
  destination: { kind: 'download' };
};
type PathLauncherRequest = GenerateLauncherRequest & {
  destination: { kind: 'path' };
};

export interface DownloadResult {
  readonly blob: Blob;
  readonly contentDisposition: string | null;
}

interface EnvelopeResponse<T> {
  readonly data?: { readonly data: T };
  readonly error?: unknown;
  readonly response: Response;
}

function unwrap<T>(result: EnvelopeResponse<T>, fallback: string): T {
  if (result.data === undefined) {
    throw responseError(result.error, result.response.status, fallback);
  }
  return result.data.data;
}

function unexpectedResponse(message: string, response: Response): ApiRequestError {
  return new ApiRequestError(message, { status: response.status });
}

export class BackendActions {
  constructor(private readonly client: ApiClient = api) {}

  async retryCamera(): Promise<OperationResult> {
    return unwrap(
      await this.client.POST('/api/camera/retry', { body: {} }),
      'camera retry failed'
    );
  }

  async saveScreenshot(request: SavedScreenshotRequest): Promise<SavedScreenshot> {
    const result = await this.client.POST('/api/camera/screenshot', { body: request });
    if (result.data === undefined) {
      throw responseError(result.error, result.response.status, 'screenshot failed');
    }
    if (Array.isArray(result.data) || !('data' in result.data)) {
      throw unexpectedResponse('screenshot returned an unexpected response', result.response);
    }
    return result.data.data;
  }

  async downloadScreenshot(request: DownloadScreenshotRequest): Promise<DownloadResult> {
    const result = await this.client.POST('/api/camera/screenshot', {
      body: request,
      parseAs: 'blob'
    });
    if (result.data === undefined) {
      throw responseError(result.error, result.response.status, 'screenshot download failed');
    }
    return {
      blob: result.data,
      contentDisposition: result.response.headers.get('Content-Disposition')
    };
  }

  async controlCommand(request: CommandControlRequest): Promise<OperationResult> {
    return unwrap(
      await this.client.POST('/api/commands/control', { body: request }),
      'command control failed'
    );
  }

  async reloadCommands(): Promise<OperationResult> {
    return unwrap(
      await this.client.POST('/api/commands/reload', { body: {} }),
      'command reload failed'
    );
  }

  async cameras(): Promise<readonly CameraDevice[]> {
    return unwrap(await this.client.GET('/api/devices/cameras'), 'camera enumeration failed');
  }

  async serialPorts(): Promise<readonly SerialPort[]> {
    return unwrap(
      await this.client.GET('/api/devices/serial-ports'),
      'serial-port enumeration failed'
    );
  }

  async controlDynamicConfig(request: DynamicConfigControlRequest): Promise<DynamicConfigResult> {
    return unwrap(
      await this.client.POST('/api/dynamic-config/control', { body: request }),
      'dynamic configuration operation failed'
    );
  }

  async testNotification(request: NotificationTestRequest): Promise<NotificationTestResult> {
    return unwrap(
      await this.client.POST('/api/notifications/test', { body: request }),
      'notification test failed'
    );
  }

  async scriptUiAction(request: ScriptUiAction): Promise<ScriptUiActionResult> {
    return unwrap(
      await this.client.POST('/api/script-ui/action', { body: request }),
      'script UI interaction failed'
    );
  }

  async generateLauncher(request: PathLauncherRequest): Promise<GenerateLauncherResult> {
    const result = await this.client.POST('/api/profiles/generate-launcher', { body: request });
    if (result.data === undefined) {
      throw responseError(result.error, result.response.status, 'launcher generation failed');
    }
    if (Array.isArray(result.data) || !('data' in result.data)) {
      throw unexpectedResponse('launcher generation returned an unexpected response', result.response);
    }
    return result.data.data;
  }

  async downloadLauncher(request: DownloadLauncherRequest): Promise<DownloadResult> {
    const result = await this.client.POST('/api/profiles/generate-launcher', {
      body: request,
      parseAs: 'blob'
    });
    if (result.data === undefined) {
      throw responseError(result.error, result.response.status, 'launcher download failed');
    }
    return {
      blob: result.data,
      contentDisposition: result.response.headers.get('Content-Disposition')
    };
  }

  async controlSerial(request: SerialControlRequest): Promise<OperationResult> {
    return unwrap(
      await this.client.POST('/api/serial/control', { body: request }),
      'serial control failed'
    );
  }

  async checkUpdate(): Promise<UpdateCheckResult> {
    return unwrap(
      await this.client.POST('/api/update/check', { body: {} }),
      'update check failed'
    );
  }
}
