import {
  ApiRequestError,
  loadSettingsSnapshot,
  patchSettingsSnapshot,
  type SettingsPatchRequest,
  type SettingsWriteValues
} from './api';
import { compareRevisions } from './revision';
import type { SettingsSnapshot } from './wire';

export interface SettingsGateway {
  load(): Promise<SettingsSnapshot>;
  patch(request: SettingsPatchRequest): Promise<SettingsSnapshot>;
}

export interface SettingsWriteResult {
  readonly recoveredRevisionConflict: boolean;
  readonly snapshot: SettingsSnapshot;
}

type SettingsSubscriber = (snapshot: SettingsSnapshot | null) => void;

const defaultGateway: SettingsGateway = {
  load: loadSettingsSnapshot,
  patch: patchSettingsSnapshot
};

/**
 * Owns the browser's optimistic settings revision and serializes all writes.
 * A conflict is recovered from exactly once by fetching a fresh snapshot and
 * replaying the caller's complete sparse patch against that revision.
 */
export class SettingsWriter {
  private snapshot: SettingsSnapshot | null = null;
  private readonly subscribers = new Set<SettingsSubscriber>();
  private tail: Promise<void> = Promise.resolve();

  constructor(private readonly gateway: SettingsGateway = defaultGateway) {}

  subscribe(subscriber: SettingsSubscriber): () => void {
    this.subscribers.add(subscriber);
    subscriber(this.snapshot);
    return () => {
      this.subscribers.delete(subscriber);
    };
  }

  acceptSnapshot(snapshot: SettingsSnapshot): void {
    if (
      this.snapshot !== null &&
      compareRevisions(snapshot.revision, this.snapshot.revision) < 0
    ) {
      return;
    }
    this.publish(snapshot);
  }

  refresh(): Promise<SettingsSnapshot> {
    return this.enqueue(async () => {
      const snapshot = await this.gateway.load();
      this.acceptSnapshot(snapshot);
      return snapshot;
    });
  }

  write(values: SettingsWriteValues): Promise<SettingsWriteResult> {
    const queuedValues = { ...values };
    return this.enqueue(() => this.performWrite(queuedValues));
  }

  private async performWrite(values: SettingsWriteValues): Promise<SettingsWriteResult> {
    const baseline = this.snapshot ?? (await this.gateway.load());
    this.acceptSnapshot(baseline);

    try {
      const snapshot = await this.gateway.patch({
        expected_revision: baseline.revision,
        values
      });
      this.acceptSnapshot(snapshot);
      return { recoveredRevisionConflict: false, snapshot };
    } catch (error: unknown) {
      if (!(error instanceof ApiRequestError) || error.code !== 'revision_conflict') {
        throw error;
      }
    }

    const refreshed = await this.gateway.load();
    this.acceptSnapshot(refreshed);
    const snapshot = await this.gateway.patch({
      expected_revision: refreshed.revision,
      values
    });
    this.acceptSnapshot(snapshot);
    return { recoveredRevisionConflict: true, snapshot };
  }

  private enqueue<T>(operation: () => Promise<T>): Promise<T> {
    const result = this.tail.then(operation);
    this.tail = result.then(
      () => undefined,
      () => undefined
    );
    return result;
  }

  private publish(snapshot: SettingsSnapshot): void {
    this.snapshot = snapshot;
    for (const subscriber of this.subscribers) {
      subscriber(snapshot);
    }
  }
}
