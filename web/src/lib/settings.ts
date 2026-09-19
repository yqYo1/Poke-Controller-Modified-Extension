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

const INSTANCE_RECOVERY_LIMIT = 3;
const RETIRED_INSTANCE_LIMIT = 32;

/**
 * Owns the browser's optimistic settings revision and serializes all writes.
 * A conflict is recovered from exactly once by fetching a fresh snapshot and
 * replaying the caller's complete sparse patch against that revision.
 */
export class SettingsWriter {
  private snapshot: SettingsSnapshot | null = null;
  private readonly retiredInstanceIds = new Set<string>();
  private readonly retiredInstanceOrder: string[] = [];
  private readonly subscribers = new Set<SettingsSubscriber>();
  private tail: Promise<void> = Promise.resolve();

  constructor(private readonly gateway: SettingsGateway = defaultGateway) {}

  subscribe(subscriber: SettingsSubscriber): () => void {
    this.subscribers.add(subscriber);
    try {
      subscriber(this.snapshot);
    } catch {
      console.error('PokeCon settings subscriber failed during initial synchronization');
    }
    return () => {
      this.subscribers.delete(subscriber);
    };
  }

  acceptSnapshot(snapshot: SettingsSnapshot): boolean {
    if (this.retiredInstanceIds.has(snapshot.instance_id)) {
      return false;
    }
    if (this.snapshot !== null) {
      if (snapshot.instance_id !== this.snapshot.instance_id) {
        this.retireInstance(this.snapshot.instance_id);
        this.publish(snapshot);
        return true;
      }
      if (compareRevisions(snapshot.revision, this.snapshot.revision) < 0) {
        return false;
      }
    }
    this.publish(snapshot);
    return true;
  }

  refresh(): Promise<SettingsSnapshot> {
    return this.enqueue(async () => {
      const snapshot = await this.gateway.load();
      if (!this.acceptSnapshot(snapshot)) {
        throw new Error('backend returned a stale settings snapshot');
      }
      return this.snapshot ?? snapshot;
    });
  }

  write(values: SettingsWriteValues): Promise<SettingsWriteResult> {
    const queuedValues = { ...values };
    return this.enqueue(() => this.performWrite(queuedValues));
  }

  private async performWrite(
    values: SettingsWriteValues,
    recoveredRevisionConflict = false,
    instanceRecoveryCount = 0
  ): Promise<SettingsWriteResult> {
    const candidate = this.snapshot ?? (await this.gateway.load());
    this.acceptSnapshot(candidate);
    const baseline = this.snapshot ?? candidate;

    let snapshot: SettingsSnapshot;
    try {
      snapshot = await this.gateway.patch({
        expected_revision: baseline.revision,
        values
      });
    } catch (error: unknown) {
      if (!(error instanceof ApiRequestError) || error.code !== 'revision_conflict') {
        throw error;
      }
      if (recoveredRevisionConflict) {
        throw error;
      }
      const refreshedCandidate = await this.gateway.load();
      this.acceptSnapshot(refreshedCandidate);
      const refreshed = this.snapshot ?? refreshedCandidate;
      snapshot = await this.gateway.patch({
        expected_revision: refreshed.revision,
        values
      });
      if (this.acceptSnapshot(snapshot)) {
        return { recoveredRevisionConflict: true, snapshot };
      }
      return this.retryAfterInstanceChange(values, true, instanceRecoveryCount);
    }

    if (this.acceptSnapshot(snapshot)) {
      return { recoveredRevisionConflict, snapshot };
    }
    return this.retryAfterInstanceChange(
      values,
      recoveredRevisionConflict,
      instanceRecoveryCount
    );
  }

  private retryAfterInstanceChange(
    values: SettingsWriteValues,
    recoveredRevisionConflict: boolean,
    instanceRecoveryCount: number
  ): Promise<SettingsWriteResult> {
    if (instanceRecoveryCount >= INSTANCE_RECOVERY_LIMIT) {
      throw new Error('backend instance changed during settings write');
    }
    return this.performWrite(values, recoveredRevisionConflict, instanceRecoveryCount + 1);
  }

  private retireInstance(instanceId: string): void {
    if (this.retiredInstanceIds.has(instanceId)) {
      return;
    }
    this.retiredInstanceIds.add(instanceId);
    this.retiredInstanceOrder.push(instanceId);
    while (this.retiredInstanceOrder.length > RETIRED_INSTANCE_LIMIT) {
      const retired = this.retiredInstanceOrder.shift();
      if (retired !== undefined) {
        this.retiredInstanceIds.delete(retired);
      }
    }
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
      try {
        subscriber(snapshot);
      } catch {
        console.error('PokeCon settings subscriber failed');
      }
    }
  }
}
