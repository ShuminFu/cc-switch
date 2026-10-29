import { vi } from "vitest";

// Kept free of app imports: the vi.mock factories load this module while
// ProviderForm itself is still being imported.
export const toastErrorMock = vi.fn();

export const sonnerMock = {
  toast: {
    error: toastErrorMock,
    success: () => {},
    warning: () => {},
    info: () => {},
  },
};

export const jsonEditorMock = {
  default: ({
    value,
    onChange,
    language,
  }: {
    value: string;
    onChange: (value: string) => void;
    language?: string;
  }) => (
    <textarea
      data-testid={`json-editor-${language ?? "json"}`}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
};
