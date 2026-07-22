import createClient from 'openapi-fetch';

import type { paths } from './generated/api';
import {
  parseSettingsSnapshot,
  parseStateSnapshot,
  type SettingsSnapshot,
  type StateSnapshot
} from './wire';

export const REQUEST_MARKER = '1';

export const api = createClient<paths>({
  baseUrl: '',
  headers: {
    'X-Pokecon-Request': REQUEST_MARKER
  }
});

export type StateEnvelope =
  paths['/api/state']['get']['responses'][200]['content']['application/json'];

export interface VisibleSnapshots {
  readonly settings: SettingsSnapshot;
  readonly state: StateSnapshot;
}

export class ApiRequestError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'ApiRequestError';
  }
}

function responseErrorMessage(error: unknown, fallback: string): string {
  if (
    typeof error === 'object' &&
    error !== null &&
    'error' in error &&
    typeof error.error === 'object' &&
    error.error !== null &&
    'message' in error.error &&
    typeof error.error.message === 'string'
  ) {
    return error.error.message;
  }
  return fallback;
}

export async function loadVisibleSnapshots(): Promise<VisibleSnapshots> {
  const [settingsResponse, stateResponse] = await Promise.all([
    api.GET('/api/settings'),
    api.GET('/api/state')
  ]);

  if (settingsResponse.data === undefined) {
    throw new ApiRequestError(
      responseErrorMessage(settingsResponse.error, 'settings snapshot request failed')
    );
  }
  if (stateResponse.data === undefined) {
    throw new ApiRequestError(
      responseErrorMessage(stateResponse.error, 'state snapshot request failed')
    );
  }

  return {
    settings: parseSettingsSnapshot(settingsResponse.data.data),
    state: parseStateSnapshot(stateResponse.data.data)
  };
}
