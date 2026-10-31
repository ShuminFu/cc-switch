import { useTranslation } from "react-i18next";
import type { DeepLinkSignature } from "./api";

/**
 * Who vouches for a deep link. Unsigned links (every v1 link) say so plainly;
 * signed links show the publisher, the key id and how the key is trusted.
 */
export function PublisherBadge({
  signature,
}: {
  signature?: DeepLinkSignature;
}) {
  const { t } = useTranslation();

  if (!signature || signature.status !== "verified") {
    return (
      <div
        className="rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-xs text-amber-800 dark:border-amber-800 dark:bg-amber-950/30 dark:text-amber-200"
        data-testid="deeplink-unsigned-badge"
      >
        <span className="font-semibold">
          {t("deeplink.unsignedLink", { defaultValue: "Unsigned link" })}
        </span>
        {" — "}
        {t("deeplink.unsignedLinkHint", {
          defaultValue:
            "anyone can create this link. Only import it if you trust where it came from.",
        })}
      </div>
    );
  }

  const known = signature.trust === "builtin" || signature.trust === "trusted";
  const mismatch = signature.trust === "mismatch";
  const tone = mismatch
    ? "border-red-300 bg-red-50 text-red-800 dark:border-red-800 dark:bg-red-950/30 dark:text-red-200"
    : known
      ? "border-green-300 bg-green-50 text-green-800 dark:border-green-800 dark:bg-green-950/30 dark:text-green-200"
      : "border-blue-300 bg-blue-50 text-blue-800 dark:border-blue-800 dark:bg-blue-950/30 dark:text-blue-200";

  return (
    <div
      className={`rounded-md border px-3 py-2 text-xs ${tone}`}
      data-testid="deeplink-signed-badge"
    >
      <div className="font-semibold">
        {known
          ? t("deeplink.signedBy", {
              defaultValue: "Signed by {{publisher}}",
              publisher: signature.publisher,
            })
          : t("deeplink.signedByUnverified", {
              defaultValue: "Signed by a key claiming to be “{{publisher}}”",
              publisher: signature.publisher,
            })}
      </div>
      <div className="mt-0.5 font-mono">
        {t("deeplink.signatureKeyId", {
          defaultValue: "Key ID {{keyId}}",
          keyId: signature.keyId ?? "?",
        })}
      </div>
      {mismatch && (
        <div className="mt-1 font-medium">
          {t("deeplink.publisherMismatch", {
            defaultValue:
              "This name belongs to a publisher you already know, but the link is signed with a different key. It may be an impersonation.",
          })}
        </div>
      )}
      {signature.trust === "new" && (
        <div className="mt-1">
          {t("deeplink.publisherNew", {
            defaultValue:
              "The signature is valid, but CC Switch has not seen this key before.",
          })}
        </div>
      )}
    </div>
  );
}
