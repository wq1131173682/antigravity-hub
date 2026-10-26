import { isTauri } from './env';

/**
 * Single IPC entry point for the whole frontend.
 *
 * This app is Tauri-only: the Rust side registers exactly one HTTP route
 * (`/*path` → the proxy handler in `modules/proxy.rs`), so there is no REST
 * surface for the UI commands to call. A previous version of this module also
 * carried a `COMMAND_MAPPING` table and a `fetch()` fallback for "web mode",
 * but no such backend ever existed — the dev server's `/api/` proxy pointed at
 * the LLM proxy port, where every one of those paths 404s. That branch was
 * unreachable in production and silently wrong in development, so it is gone
 * (along with the dead `/api/` proxy entry in `vite.config.ts`).
 *
 * Calling this outside Tauri now fails loudly instead of issuing a request
 * that cannot succeed.
 */
export async function request<T>(cmd: string, args?: unknown): Promise<T> {
  if (!isTauri()) {
    throw new Error(
      `IPC command [${cmd}] requires the Tauri runtime. ` +
      'This app has no web/HTTP mode — run it via `npm run tauri dev` or the built desktop app.',
    );
  }

  try {
    const { invoke } = await import('@tauri-apps/api/core');
    return await invoke<T>(cmd, args as Record<string, unknown> | undefined);
  } catch (error) {
    console.error(`Tauri Invoke Error [${cmd}]:`, error);
    throw error;
  }
}
