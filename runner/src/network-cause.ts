// Fetch's useful error is usually in its cause, or in an AggregateError
// from trying several addresses. Only codes go into the public message:
// cause messages and addresses can contain private connection details.
const NETWORK_CODES = new Set([
  'ECONNRESET', 'ECONNREFUSED', 'ECONNABORTED', 'ENOTFOUND', 'EAI_AGAIN',
  'ETIMEDOUT', 'EHOSTUNREACH', 'ENETUNREACH', 'ENETDOWN', 'EADDRNOTAVAIL', 'EPROTO', 'EPIPE',
  'UND_ERR_CONNECT_TIMEOUT', 'UND_ERR_HEADERS_TIMEOUT',
  'UND_ERR_BODY_TIMEOUT', 'UND_ERR_SOCKET', 'UND_ERR_DESTROYED', 'UND_ERR_CLOSED', 'UND_ERR_ABORTED',
  'CERT_HAS_EXPIRED', 'DEPTH_ZERO_SELF_SIGNED_CERT',
  'UNABLE_TO_VERIFY_LEAF_SIGNATURE', 'ERR_TLS_CERT_ALTNAME_INVALID',
  'SELF_SIGNED_CERT_IN_CHAIN', 'UNABLE_TO_GET_ISSUER_CERT_LOCALLY',
]);

export function withNetworkCause(error: unknown): unknown {
  try {
    if (!(error instanceof Error)) return error;
    const pending: unknown[] = [error];
    const seen = new Set<object>();
    const codes = new Set<string>();
    for (let i = 0; i < pending.length && i < 16 && codes.size < 3; i++) {
      const item = pending[i];
      if (!item || typeof item !== 'object' || seen.has(item)) continue;
      seen.add(item);
      const e = item as { code?: unknown; name?: unknown; cause?: unknown; errors?: unknown };
      if (typeof e.code === 'string' && NETWORK_CODES.has(e.code)) codes.add(e.code);
      if (codes.size < 3 && (e.name === 'TimeoutError' || e.name === 'AbortError')) codes.add(e.name);
      if (e.cause) pending.push(e.cause);
      if (Array.isArray(e.errors)) pending.push(...e.errors.slice(0, 4));
    }
    return codes.size ? new Error(`${error.message} (${[...codes].join(', ')})`, { cause: error }) : error;
  } catch {
    // A diagnostic must never replace the failure it's meant to explain.
    return error;
  }
}
