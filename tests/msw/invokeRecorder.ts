import { http, HttpResponse } from "msw";
import { server } from "./server";

const TAURI_ENDPOINT = "http://tauri.local";

export interface RecordedInvoke {
  command: string;
  args: Record<string, unknown>;
}

type Responder =
  | unknown
  | ((args: Record<string, unknown>) => unknown | Promise<unknown>);

/**
 * Per-test override for one or more Tauri commands, routed through the real
 * `invoke` → MSW bridge (tests/msw/tauriMocks.ts) so the component's API
 * wrapper (`src/lib/api/*`) runs unmodified.
 *
 * A responder may be a value or a function of the invoke payload. Throwing
 * an `InvokeError` (or any Error) makes the invoke reject with that message,
 * which is how a Rust `Err(String)` surfaces in the frontend.
 */
export function mockInvoke(responders: Record<string, Responder>) {
  const calls: RecordedInvoke[] = [];
  server.use(
    ...Object.entries(responders).map(([command, responder]) =>
      http.post(`${TAURI_ENDPOINT}/${command}`, async ({ request }) => {
        const text = await request.text();
        const args = (text ? JSON.parse(text) : {}) as Record<string, unknown>;
        calls.push({ command, args });
        try {
          const value =
            typeof responder === "function"
              ? await (responder as (a: Record<string, unknown>) => unknown)(
                  args,
                )
              : responder;
          return HttpResponse.json((value ?? null) as never);
        } catch (error) {
          const message =
            error instanceof Error ? error.message : String(error);
          return new HttpResponse(message, { status: 500 });
        }
      }),
    ),
  );
  return {
    calls,
    callsTo: (command: string) =>
      calls.filter((call) => call.command === command),
  };
}
