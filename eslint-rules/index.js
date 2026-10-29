// Local ESLint rules for the cc-switch frontend (registered as the "cc-switch" plugin
// in eslint.config.js).

// Keep in sync with APP_REGISTRY in src/entities/app/registry.ts;
// tests/lint/eslintConfig.test.ts checks the two lists match.
export const APP_IDS = [
  "claude",
  "claude-desktop",
  "codex",
  "gemini",
  "grokbuild",
  "opencode",
  "openclaw",
  "hermes",
];

const APP_ID_SET = new Set(APP_IDS);

const COMPARISON_OPERATORS = new Set(["===", "!==", "==", "!="]);

/** Returns the app id if `node` is a string literal (or a plain template) naming one. */
const appIdLiteral = (node) => {
  if (node.type === "Literal" && typeof node.value === "string") {
    return APP_ID_SET.has(node.value) ? node.value : null;
  }
  if (node.type === "TemplateLiteral" && node.expressions.length === 0) {
    const value = node.quasis[0].value.cooked;
    return APP_ID_SET.has(value) ? value : null;
  }
  return null;
};

const isLiteralLike = (node) =>
  node.type === "Literal" ||
  (node.type === "TemplateLiteral" && node.expressions.length === 0);

/**
 * Flags `x === "claude"`-style comparisons and `case "claude":` labels.
 * Existing ones are recorded in eslint-suppressions.json; new code should ask the
 * app registry (useAppCaps / appCaps / appLabel) instead of branching on app ids.
 */
const noAppLiteralComparison = {
  meta: {
    type: "suggestion",
    docs: {
      description:
        "Disallow comparing against app-id string literals; use the app registry instead",
    },
    schema: [],
    messages: {
      comparison:
        'Comparison against the app id "{{id}}". Use the app registry (src/entities/app/registry.ts) instead of app-id literals.',
    },
  },
  create(context) {
    return {
      BinaryExpression(node) {
        if (!COMPARISON_OPERATORS.has(node.operator)) return;
        // "claude" === "claude" is not a branch on an app id.
        if (isLiteralLike(node.left) && isLiteralLike(node.right)) return;
        const id = appIdLiteral(node.left) ?? appIdLiteral(node.right);
        if (id) {
          context.report({ node, messageId: "comparison", data: { id } });
        }
      },
      SwitchCase(node) {
        if (!node.test) return;
        const id = appIdLiteral(node.test);
        if (id) {
          context.report({
            node: node.test,
            messageId: "comparison",
            data: { id },
          });
        }
      },
    };
  },
};

export default {
  meta: { name: "eslint-plugin-cc-switch" },
  rules: {
    "no-app-literal-comparison": noAppLiteralComparison,
  },
};
