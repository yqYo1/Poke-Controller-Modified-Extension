import createClient from 'openapi-fetch';

import type { paths } from './generated/api';

export const REQUEST_MARKER = '1';

export const api = createClient<paths>({
  baseUrl: '',
  headers: {
    'X-Pokecon-Request': REQUEST_MARKER
  }
});

export type StateEnvelope =
  paths['/api/state']['get']['responses'][200]['content']['application/json'];
