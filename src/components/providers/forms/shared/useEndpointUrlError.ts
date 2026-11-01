import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ENDPOINT_URL_ERROR_MESSAGES,
  validateEndpointUrl,
} from "@/utils/providerInputValidation";

/**
 * Inline endpoint validation shared by the endpoint inputs of every app
 * (FE-PROV-21): the message appears after the field was left once, so the
 * user is not scolded while typing. Empty values are left to the form's
 * soft validation.
 */
export function useEndpointUrlError(value: string) {
  const { t } = useTranslation();
  const [touched, setTouched] = useState(false);
  const code = useMemo(
    () => validateEndpointUrl(value, { allowEmpty: true }),
    [value],
  );
  const message =
    touched && code
      ? t(ENDPOINT_URL_ERROR_MESSAGES[code].key, {
          defaultValue: ENDPOINT_URL_ERROR_MESSAGES[code].defaultValue,
        })
      : null;
  return {
    message,
    inputProps: {
      onBlur: () => setTouched(true),
      "aria-invalid": message ? true : undefined,
      className: message
        ? "border-destructive focus-visible:ring-destructive"
        : undefined,
    },
  };
}
