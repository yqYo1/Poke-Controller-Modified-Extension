import createClient from 'openapi-fetch';

import type { components, paths } from './api/openapi';
import {
  parseSettingsSnapshot,
  parseStateSnapshot,
  type SettingsSnapshot,
  type StateSnapshot
} from './wire';

export const REQUEST_MARKER = '1';

export function createPokeconApi(
  request?: (input: Request) => Promise<Response>,
  baseUrl = ''
): ReturnType<typeof createClient<paths>> {
  return createClient<paths>({
    baseUrl,
    ...(request === undefined ? {} : { fetch: request }),
    headers: {
      'X-Pokecon-Request': REQUEST_MARKER
    }
  });
}

export const api = createPokeconApi();
export type ApiClient = typeof api;

export type StateEnvelope =
  paths['/api/state']['get']['responses'][200]['content']['application/json'];
export type ApiErrorCode = components['schemas']['ApiErrorCode'];
export type ApiErrorFields = components['schemas']['ApiError']['fields'];
export type SettingsPatchRequest = components['schemas']['SettingsPatchRequest'];
export type SettingsWriteValues = components['schemas']['SettingsWriteValues'];

export interface VisibleSnapshots {
  readonly settings: SettingsSnapshot;
  readonly state: StateSnapshot;
}

export class ApiRequestError extends Error {
  readonly code: ApiErrorCode | null;
  readonly fields: ApiErrorFields;
  readonly status: number;

  constructor(
    message: string,
    options: {
      readonly code?: ApiErrorCode | null;
      readonly fields?: ApiErrorFields;
      readonly status?: number;
    } = {}
  ) {
    super(message);
    this.name = 'ApiRequestError';
    this.code = options.code ?? null;
    this.fields = options.fields ?? null;
    this.status = options.status ?? 0;
  }
}

export function responseError(
  error: unknown,
  status: number,
  fallback: string
): ApiRequestError {
  if (
    typeof error === 'object' &&
    error !== null &&
    'error' in error &&
    typeof error.error === 'object' &&
    error.error !== null &&
    'message' in error.error &&
    typeof error.error.message === 'string'
  ) {
    const code =
      'code' in error.error && typeof error.error.code === 'string'
        ? (error.error.code as ApiErrorCode)
        : null;
    const fields =
      'fields' in error.error &&
      (error.error.fields === null || typeof error.error.fields === 'object')
        ? (error.error.fields as ApiErrorFields)
        : null;
    return new ApiRequestError(error.error.message, { code, fields, status });
  }
  return new ApiRequestError(fallback, { status });
}

export async function loadSettingsSnapshot(): Promise<SettingsSnapshot> {
  const response = await api.GET('/api/settings');
  if (response.data === undefined) {
    throw responseError(response.error, response.response.status, 'settings snapshot request failed');
  }
  return parseSettingsSnapshot(response.data.data);
}

export async function patchSettingsSnapshot(
  request: SettingsPatchRequest
): Promise<SettingsSnapshot> {
  const response = await api.PATCH('/api/settings', { body: request });
  if (response.data === undefined) {
    throw responseError(response.error, response.response.status, 'settings update failed');
  }
  return parseSettingsSnapshot(response.data.data);
}

export async function loadStateSnapshot(): Promise<StateSnapshot> {
  const response = await api.GET('/api/state');
  if (response.data === undefined) {
    throw responseError(response.error, response.response.status, 'state snapshot request failed');
  }
  return parseStateSnapshot(response.data.data);
}

export async function loadVisibleSnapshots(): Promise<VisibleSnapshots> {
  const [settings, state] = await Promise.all([loadSettingsSnapshot(), loadStateSnapshot()]);

  return { settings, state };
}
