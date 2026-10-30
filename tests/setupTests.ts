import "@testing-library/jest-dom";
import { afterAll, afterEach, beforeAll, vi } from "vitest";
import { cleanup } from "@testing-library/react";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { server } from "./msw/server";
import { resetProviderState } from "./msw/state";
import "./msw/tauriMocks";

// Every invoke that reaches MSW without a handler fails the test that made it
// (TEST-40). With `"warn"` the invoke merely rejected and components rendered
// their error branch while assertions still passed. Add a handler in
// tests/msw/handlers.ts, or a per-test override via tests/msw/invokeRecorder.ts.
const unhandledInvokes: string[] = [];

beforeAll(async () => {
  server.listen({
    onUnhandledRequest(request, print) {
      const url = new URL(request.url);
      if (url.host === "tauri.local") {
        unhandledInvokes.push(url.pathname.replace(/^\//, ""));
      }
      print.warning();
    },
  });
  await i18n.use(initReactI18next).init({
    lng: "zh",
    fallbackLng: "zh",
    resources: {
      zh: { translation: {} },
      en: { translation: {} },
    },
    interpolation: {
      escapeValue: false,
    },
  });
});

afterEach(() => {
  cleanup();
  resetProviderState();
  server.resetHandlers();
  vi.clearAllMocks();
  const unhandled = unhandledInvokes.splice(0);
  if (unhandled.length > 0) {
    throw new Error(
      `Unmocked Tauri invoke(s): ${[...new Set(unhandled)].join(", ")}. ` +
        "Add a handler in tests/msw/handlers.ts or mock it in the test.",
    );
  }
});

afterAll(() => {
  server.close();
});
