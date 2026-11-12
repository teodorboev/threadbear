// Pools shared between runners through the backend (docs/protocol.md,
// "Pools", "Shared between runners"). This follows the backend's list,
// keeps the last one in shared-pools.json so a start with no network still
// has it, puts this runner's runner.json pools up the first time when the
// list is empty, and writes pools.save there. The translation between the
// shared list and this runner's own pools is in pools.ts.
import fs from 'node:fs';
import type { Account, Pool } from './protocol.ts';
import { fromShared, readShared, toShared, type SharedPool } from './pools.ts';
import type { Logger } from './log.ts';

export type PoolBackend = {
  subscribe(onRows: (rows: SharedPool[]) => void, onError: (error: Error) => void): () => void;
  save(pools: SharedPool[]): Promise<void>;
  // True when these went in: only into an empty list.
  adopt(pools: SharedPool[]): Promise<boolean>;
  // The backend takes a member with both an email and an id (two logins
  // here sharing an email, pools.ts memberOf). False or missing: email only.
  readonly pins?: boolean;
};

export type SharedPoolsDeps = {
  cachePath: string;
  runnerId: string;
  accounts: () => Account[];
  // runner.json's own pools, when it has any.
  localPools: () => Pool[] | undefined;
  // The list changed: the runner works its pools out again.
  onChange: () => void;
  log: Pick<Logger, 'info' | 'warn'>;
};

export class SharedPools {
  // The backend's list; undefined while it has none (each runner keeps
  // its own pools until then) or before it's been heard from.
  private list?: SharedPool[];
  private backend?: PoolBackend;
  private unsubscribe?: () => void;
  // The backend has said its list is empty.
  private empty = false;
  private adopting = false;

  private deps: SharedPoolsDeps;

  constructor(deps: SharedPoolsDeps) {
    this.deps = deps;
  }

  // The copy from last time, before the backend is heard from.
  loadCache(): void {
    try {
      const rows = readShared(JSON.parse(fs.readFileSync(this.deps.cachePath, 'utf8')));
      if (rows.length) this.list = rows;
    } catch {
      // none yet
    }
  }

  // From sync's heartbeat once the backend is API 38 or later, so it's
  // called again and again: the first call subscribes, and later ones
  // retry putting runner.json's pools up if that couldn't happen yet.
  attach(backend: PoolBackend): void {
    if (this.backend) {
      void this.maybeAdopt();
      return;
    }
    this.backend = backend;
    this.unsubscribe = backend.subscribe(
      (rows) => this.receive(rows),
      (error) => this.deps.log.warn('shared pools subscription failed', { error: String(error) }),
    );
  }

  get attached(): boolean {
    return !!this.backend;
  }

  stop(): void {
    this.unsubscribe?.();
  }

  // This runner's pools from the shared list, or undefined to use its own:
  // there's no shared list, or none of its logins are in it (a runner with
  // no pools couldn't start a thread).
  pools(accounts: Account[]): Pool[] | undefined {
    if (!this.list?.length) return undefined;
    const pools = fromShared(this.list, accounts, this.deps.runnerId);
    return pools.length ? pools : undefined;
  }

  // pools.save while attached: the new shared list, keeping what this
  // runner can't see (toShared).
  async save(pools: Pool[]): Promise<void> {
    if (!this.backend) throw new Error('not attached to the backend');
    const accounts = this.deps.accounts();
    const local = this.deps.localPools();
    const pin = !!this.backend.pins;
    const previous = this.list ?? (local?.length ? toShared(local, [], accounts, this.deps.runnerId, pin) : []);
    const next = toShared(pools, previous, accounts, this.deps.runnerId, pin);
    await this.backend.save(next);
    this.set(next);
  }

  private receive(rows: SharedPool[]): void {
    const list = readShared(rows);
    if (list.length) {
      this.empty = false;
      this.set(list);
      return;
    }
    this.empty = true;
    if (this.list) {
      this.list = undefined;
      fs.rmSync(this.deps.cachePath, { force: true });
      this.deps.onChange();
    }
    void this.maybeAdopt();
  }

  private set(list: SharedPool[]): void {
    const changed = JSON.stringify(list) !== JSON.stringify(this.list);
    this.list = list;
    if (!changed) return;
    try {
      fs.writeFileSync(this.deps.cachePath, JSON.stringify(list, null, 2) + '\n');
    } catch (e) {
      this.deps.log.warn('could not keep the shared pools', { error: String(e) });
    }
    this.deps.onChange();
  }

  // The backend's list is empty and runner.json has pools: they go up, as
  // long as every login in them has an email (one without would be shared
  // as this runner's only, which another runner can't use, so it waits for
  // the accounts to be read).
  private async maybeAdopt(): Promise<void> {
    if (!this.backend || !this.empty || this.adopting) return;
    const local = this.deps.localPools();
    if (!local?.length) return;
    const accounts = this.deps.accounts();
    const used = new Set(local.flatMap((p) => p.accountIds));
    if (accounts.some((a) => used.has(a.id) && !a.email)) return;
    this.adopting = true;
    try {
      const took = await this.backend.adopt(toShared(local, [], accounts, this.deps.runnerId, !!this.backend.pins));
      if (took) this.deps.log.info('shared pools: put this runner\'s pools up', { count: local.length });
    } catch (e) {
      this.deps.log.warn('could not put the pools up', { error: String(e) });
    } finally {
      this.adopting = false;
    }
  }
}
