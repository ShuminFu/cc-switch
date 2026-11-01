import { invoke } from "@tauri-apps/api/core";

/** Mirrors `features/policy.rs` (PRODUCT-24). */
export type ManagedPolicyState =
  | "none"
  | "active"
  | "invalid"
  | "signatureInvalid";

export interface ManagedPolicy {
  version: number;
  name?: string;
  message?: string;
  endpoints: { allow: string[]; deny: string[] };
  disabledFeatures: string[];
}

export interface ManagedPolicyStatus {
  state: ManagedPolicyState;
  source?: string;
  signed: boolean;
  policy?: ManagedPolicy;
  error?: string;
  unknownFeatures: string[];
}

export const policyApi = {
  getStatus(): Promise<ManagedPolicyStatus> {
    return invoke("get_managed_policy_status");
  },
};
