import { useState } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { useForm } from "react-hook-form";
import { describe, expect, it } from "vitest";
import {
  normalizeApiKey,
  validateEndpointUrl,
} from "@/utils/providerInputValidation";
import { EndpointField } from "@/components/providers/forms/shared/EndpointField";
import ApiKeyInput from "@/components/providers/forms/ApiKeyInput";
import { Form } from "@/components/ui/form";

describe("validateEndpointUrl (FE-PROV-21)", () => {
  it("accepts http(s) URLs and template tokens", () => {
    expect(validateEndpointUrl("https://api.example.com/v1")).toBeNull();
    expect(
      validateEndpointUrl(
        "https://bedrock-runtime.${AWS_REGION}.amazonaws.com",
      ),
    ).toBeNull();
  });

  it("rejects typos and other schemes", () => {
    expect(validateEndpointUrl("htp://x")).toBe("scheme");
    expect(validateEndpointUrl("api.example.com")).toBe("invalid");
    expect(validateEndpointUrl("ftp://example.com")).toBe("scheme");
    expect(validateEndpointUrl("  ")).toBe("empty");
    expect(validateEndpointUrl("", { allowEmpty: true })).toBeNull();
  });
});

describe("normalizeApiKey (FE-PROV-21 / FE-PROV-36)", () => {
  it("strips quotes, Bearer prefixes and line breaks", () => {
    expect(normalizeApiKey('"sk-abc"')).toBe("sk-abc");
    expect(normalizeApiKey("'sk-abc'\n")).toBe("sk-abc");
    expect(normalizeApiKey("Bearer sk-abc")).toBe("sk-abc");
    expect(normalizeApiKey("  sk-abc\r\n")).toBe("sk-abc");
    expect(normalizeApiKey("sk-abc")).toBe("sk-abc");
  });
});

function EndpointHarness() {
  const [value, setValue] = useState("");
  const form = useForm();
  return (
    <Form {...form}>
      <EndpointField
        id="endpoint"
        label="Endpoint"
        value={value}
        onChange={setValue}
        placeholder="https://"
      />
    </Form>
  );
}

function KeyHarness() {
  const [value, setValue] = useState("");
  return <ApiKeyInput value={value} onChange={setValue} id="key" />;
}

describe("inline endpoint error and key paste", () => {
  it("shows an inline error for htp://x after leaving the field", () => {
    render(<EndpointHarness />);
    const input = screen.getByPlaceholderText("https://");
    fireEvent.change(input, { target: { value: "htp://x" } });
    expect(input).not.toHaveAttribute("aria-invalid");
    fireEvent.blur(input);
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(
      screen.getByText(/http:\/\/ or https:\/\/|endpointScheme/),
    ).toBeInTheDocument();
  });

  it('stores sk-abc when "sk-abc" is pasted with quotes', () => {
    render(<KeyHarness />);
    const input = document.getElementById("key") as HTMLInputElement;
    fireEvent.paste(input, {
      clipboardData: { getData: () => '"sk-abc"\n' },
    });
    expect(input.value).toBe("sk-abc");
  });
});
