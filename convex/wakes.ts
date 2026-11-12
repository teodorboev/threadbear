// scheduleWakes: the small rows the website's sidebar reads instead of the
// schedules themselves (schema.ts). A row exists while its schedule will run by
// itself: not paused, not finished.
import type { MutationCtx } from './_generated/server';
import { isIsoTime } from './runnerAccess';

type ScheduleFields = { id: string; runnerId: string; threadId?: string; nextRunAt?: string; paused: boolean; finishedAt?: string };

export async function syncWake(ctx: { db: MutationCtx['db'] }, s: ScheduleFields): Promise<void> {
  const row = await ctx.db.query('scheduleWakes').withIndex('by_scheduleId', (q) => q.eq('scheduleId', s.id)).unique();
  if (s.paused || s.finishedAt) {
    if (row) await ctx.db.delete(row._id);
    return;
  }
  // Only a time the apps can read: the phones draw it, and Android's took
  // "+1000000000-06-01T00:00:00Z" from a runner key's schedule and threw
  // drawing the inbox (Daybreak, 11 Nov 2026). Without one the row still
  // sorts as scheduled, just with no time beside it.
  const want = { scheduleId: s.id, runnerId: s.runnerId, threadId: s.threadId, nextRunAt: isIsoTime(s.nextRunAt) ? s.nextRunAt : undefined };
  if (!row) await ctx.db.insert('scheduleWakes', want);
  // Unchanged is the usual case, and a write would re-run every open sidebar.
  else if (row.runnerId !== want.runnerId || row.threadId !== want.threadId || row.nextRunAt !== want.nextRunAt) await ctx.db.replace(row._id, want);
}

export async function dropWake(ctx: { db: MutationCtx['db'] }, scheduleId: string): Promise<void> {
  const row = await ctx.db.query('scheduleWakes').withIndex('by_scheduleId', (q) => q.eq('scheduleId', scheduleId)).unique();
  if (row) await ctx.db.delete(row._id);
}

// A runner's schedules from before this table, the first heartbeat it's seen.
export async function seedWakes(ctx: { db: MutationCtx['db'] }, runnerId: string): Promise<void> {
  for await (const s of ctx.db.query('schedules').withIndex('by_runnerId', (q) => q.eq('runnerId', runnerId))) await syncWake(ctx, s);
}
