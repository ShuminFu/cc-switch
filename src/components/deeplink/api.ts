import { invoke } from "@tauri-apps/api/core";
import type { DeepLinkImportRequest } from "@/lib/api/deeplink";

/** How a signed link's key relates to known publishers (SESS-10). */
export type PublisherTrust = "builtin" | "trusted" | "new" | "mismatch";

export interface DeepLinkSignature {
  status: "unsigned" | "verified";
  publisher?: string;
  keyId?: string;
  trust?: PublisherTrust;
  publicKey?: string;
}

export interface DeepLinkBundle {
  version: number;
  items: DeepLinkImportRequestV2[];
  signature: DeepLinkSignature;
  minApp?: string;
}

/** The request emitted by the backend: v1 fields plus the pending-import
 * nonce and, for v2 links, the whole bundle. */
export type DeepLinkImportRequestV2 = DeepLinkImportRequest & {
  nonce?: string;
  bundle?: DeepLinkBundle;
};

export interface BundleItemChoice {
  index: number;
  accept: boolean;
  enabled?: boolean;
  usageEnabled?: boolean;
}

export interface BundleItemResult {
  index: number;
  resource: string;
  ok: boolean;
  result?: Record<string, unknown>;
  error?: string;
}

export const deeplinkV2Api = {
  /** Import the accepted items of a pending link by its nonce. */
  importBundle(
    nonce: string,
    choices: BundleItemChoice[],
    trustPublisher: boolean,
  ): Promise<BundleItemResult[]> {
    return invoke("import_deeplink_bundle", {
      nonce,
      choices,
      trustPublisher,
    });
  },

  /** Forget a pending link the user dismissed. */
  discard(nonce: string): Promise<boolean> {
    return invoke("discard_pending_deeplink", { nonce });
  },

  /** Build a `ccswitch://v2/import` link for a configured resource. */
  buildShareLink(options: {
    resource: "provider" | "mcp" | "prompt";
    app: string;
    id: string;
    includeSecrets: boolean;
  }): Promise<string> {
    return invoke("build_deeplink_share_link", options);
  },
};

export const isBundle = (request: DeepLinkImportRequestV2 | null) =>
  !!request?.bundle && request.bundle.items.length > 1;
