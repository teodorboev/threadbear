// A thread id's shape: 1 to 64 letters, digits, `-` and `_`. This runner
// makes UUIDs; the mock and fixtures use words like demo_upload. An id from
// outside (another runner's, through the backend or a move) is checked with
// this before it's kept or used: the apps put ids into a shell line (Copy
// debug info) and a chip's link, and this runner into paths. The backend
// (convex/runnerAccess.ts isThreadId) and the apps (ThreadIds.isValid,
// thread_link::is_thread_id) check the same (Daybreak 87 and 102, 11 Nov 2026).
const THREAD_ID = /^[A-Za-z0-9_-]{1,64}$/;

export function isThreadIdShape(id: unknown): id is string {
  return typeof id === 'string' && THREAD_ID.test(id);
}
