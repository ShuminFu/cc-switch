import { render, screen, fireEvent } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ManagedPolicyBannerView } from "@/components/policy/ManagedPolicyBanner";

describe("ManagedPolicyBannerView (PRODUCT-24)", () => {
  it("shows the policy name and expands to the enforced rules", () => {
    render(
      <ManagedPolicyBannerView
        status={{
          state: "active",
          signed: true,
          source: "/etc/cc-switch/policy.json",
          unknownFeatures: [],
          policy: {
            version: 1,
            name: "Acme IT",
            endpoints: { allow: [], deny: ["*.example-relay.com"] },
            disabledFeatures: ["deeplinkImport"],
          },
        }}
      />,
    );
    expect(screen.getByText(/Acme IT/)).toBeInTheDocument();
    expect(screen.queryByText("*.example-relay.com")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button"));
    expect(screen.getByText("*.example-relay.com")).toBeInTheDocument();
    expect(screen.getByText("deeplinkImport")).toBeInTheDocument();
  });

  it("warns when a policy file is ignored for a bad signature", () => {
    render(
      <ManagedPolicyBannerView
        status={{
          state: "signatureInvalid",
          signed: false,
          source: "/etc/cc-switch/policy.json",
          error: "signature mismatch",
          unknownFeatures: [],
        }}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(/signature/i);
    expect(screen.getByText("signature mismatch")).toBeInTheDocument();
  });
});
