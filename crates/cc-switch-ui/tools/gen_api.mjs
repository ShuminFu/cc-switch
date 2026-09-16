// Generates crates/cc-switch-ui/src/api/*.rs from the React API layer
// (src/lib/api/*.ts) using the TypeScript compiler API. Migration-time
// scaffolding: run it while the TS modules are still the reference, then
// hand-edit the Rust as views are ported and types land in the contract crate.
//
//   node crates/cc-switch-ui/tools/gen_api.mjs
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const ts = require("typescript");
const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, "../../..");
const apiDir = path.join(repo, "src/lib/api");
const outDir = path.join(repo, "crates/cc-switch-ui/src/api");

const KNOWN = {
  string: "String", number: "f64", boolean: "bool", void: "()", any: "serde_json::Value", unknown: "serde_json::Value",
  AppId: "AppId", AppType: "AppId", Provider: "Provider", Settings: "AppSettings", ProviderMeta: "ProviderMeta",
};
const snake = (s) => s.replace(/([a-z0-9])([A-Z])/g, "$1_$2").replace(/-/g, "_").toLowerCase();
const constName = (s) => snake(s).toUpperCase();

function rustType(t, { param } = {}) {
  t = t.trim().replace(/^\((.*)\)$/, "$1");
  const orNull = /\s*\|\s*(null|undefined)$/.test(t);
  if (orNull) return `Option<${rustType(t.replace(/\s*\|\s*(null|undefined)$/, ""), { param })}>`;
  if (t.endsWith("[]")) return `Vec<${rustType(t.slice(0, -2))}>`;
  let m = t.match(/^Array<(.*)>$/); if (m) return `Vec<${rustType(m[1])}>`;
  m = t.match(/^Record<string,\s*(.*)>$/); if (m) return `HashMap<String, ${rustType(m[1])}>`;
  if (t in KNOWN) return KNOWN[t];
  if (/^"/.test(t)) return "String"; // string literal unions
  return "serde_json::Value";
}
function paramType(t) {
  const r = rustType(t, { param: true });
  if (r === "String") return "&str";
  if (r === "serde_json::Value" || r.startsWith("Vec<") || r.startsWith("HashMap<") || ["Provider", "AppSettings", "ProviderMeta"].includes(r)) return `&${r}`;
  return r;
}

function header(module) {
  return `//! Generated from src/lib/api/${module}.ts by tools/gen_api.mjs.
//! Untyped payloads are \`serde_json::Value\` until the contract crate types them.
#![allow(dead_code, unused_imports, clippy::too_many_arguments)]

use std::collections::HashMap;

use cc_switch_contract::commands::${snake(module)} as cmd;
use cc_switch_contract::{AppId, AppSettings, IpcError, Provider, ProviderMeta};
use serde_json::json;

use crate::ipc;

`;
}

const modules = [];
for (const file of fs.readdirSync(apiDir).filter((f) => f.endsWith(".ts") && !["index.ts", "types.ts"].includes(f))) {
  const module = file.replace(/\.ts$/, "");
  const src = fs.readFileSync(path.join(apiDir, file), "utf8");
  const sf = ts.createSourceFile(file, src, ts.ScriptTarget.Latest, true);
  let out = header(module);
  let count = 0;
  const seen = new Set();
  const emit = (name, params, retType, cmdName, argProps) => {
    let fnName = snake(name);
    while (seen.has(fnName)) fnName += "_";
    seen.add(fnName);
    const rparams = params.map((p) => `${snake(p.name)}: ${p.optional ? `Option<${rustType(p.type)}>` : paramType(p.type)}`).join(", ");
    const ret = rustType(retType);
    let call;
    if (argProps === null) call = `ipc::invoke_no_args(cmd::${constName(cmdName)}).await`;
    else {
      const props = argProps.map(([k, v]) => `"${k}": ${snake(v)}`).join(", ");
      call = `ipc::invoke(cmd::${constName(cmdName)}, &json!({ ${props} })).await`;
    }
    out += `pub async fn ${fnName}(${rparams}) -> Result<${ret}, IpcError> {\n    ${call}\n}\n\n`;
    count++;
  };
  const visit = (node) => {
    if (ts.isMethodDeclaration(node) && node.body) {
      const name = node.name.getText(sf);
      const params = node.parameters.map((p) => ({ name: p.name.getText(sf), type: p.type ? p.type.getText(sf) : "unknown", optional: !!p.questionToken || !!p.initializer }));
      let retType = "unknown";
      if (node.type) { const m = node.type.getText(sf).match(/^Promise<(.*)>$/s); retType = m ? m[1] : node.type.getText(sf); }
      const invokes = [];
      const walk = (n) => {
        if (ts.isCallExpression(n) && n.expression.getText(sf) === "invoke" && n.arguments.length && ts.isStringLiteral(n.arguments[0])) {
          const cmdName = n.arguments[0].text;
          let argProps = null;
          if (n.arguments[1] && ts.isObjectLiteralExpression(n.arguments[1])) {
            argProps = n.arguments[1].properties.map((pr) => {
              if (ts.isShorthandPropertyAssignment(pr)) return [pr.name.text, pr.name.text];
              if (ts.isPropertyAssignment(pr)) return [pr.name.getText(sf).replace(/["']/g, ""), pr.initializer.getText(sf)];
              return ["__spread", "value"];
            });
          } else if (n.arguments[1]) argProps = [["__raw", n.arguments[1].getText(sf)]];
          invokes.push({ cmdName, argProps });
        }
        ts.forEachChild(n, walk);
      };
      walk(node.body);
      if (invokes.length === 1) {
        const { cmdName, argProps } = invokes[0];
        // Only emit when every arg is a plain identifier we can map to a param.
        const paramNames = new Set(params.map((p) => p.name));
        const simple = argProps === null || argProps.every(([, v]) => /^[A-Za-z_][A-Za-z0-9_]*$/.test(v) && paramNames.has(v));
        if (simple) emit(name, params, retType, cmdName, argProps);
        else out += `// TODO(port): ${name}(${params.map((p) => p.name + ": " + p.type).join(", ")}) -> ${retType}: invoke("${cmdName}", ${JSON.stringify(argProps)})`.replace(/\s+/g, " ") + "\n\n";
      } else if (invokes.length > 1) {
        out += `// TODO(port): ${name} invokes several commands: ${invokes.map((i) => i.cmdName).join(", ")}\n\n`;
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(sf);
  fs.writeFileSync(path.join(outDir, `${snake(module)}.rs`), out);
  modules.push([snake(module), count]);
}
fs.writeFileSync(path.join(outDir, "mod.rs"), `//! Typed wrappers over the Tauri commands, one module per backend area.\n//! Generated by tools/gen_api.mjs from src/lib/api/*.ts; hand-edited as\n//! views are ported. \`app\` is hand-written.\n\npub mod app;\n${modules.map(([m]) => `pub mod ${m};`).join("\n")}\n`);
console.log(modules.map(([m, c]) => `${m}: ${c}`).join("\n"));
