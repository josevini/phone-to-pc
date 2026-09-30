// Fails unless every npm package in package-lock.json has a permissive licence, the npm counterpart of
// src-tauri/deny.toml. The window ships no npm package: these are build and test tools.
import { readFileSync } from "node:fs";

const allowed = new Set([
  "0BSD",
  "Apache-2.0",
  "BlueOak-1.0.0",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "CC0-1.0",
  "ISC",
  "MIT",
  "Unlicense",
  "Zlib",
]);

/** Whether an SPDX expression is satisfied by allowed licences: "A OR B" needs one of them, "A AND B" both. */
function permissive(expression) {
  const inner = expression.replace(/^\((.*)\)$/, "$1");
  if (inner.includes(" OR ")) return inner.split(" OR ").some(permissive);
  if (inner.includes(" AND ")) return inner.split(" AND ").every(permissive);
  return allowed.has(inner.trim());
}

const lock = JSON.parse(readFileSync(new URL("../package-lock.json", import.meta.url), "utf8"));
const rejected = Object.entries(lock.packages)
  .filter(([path]) => path !== "")
  .filter(([, pkg]) => !permissive(pkg.license ?? ""))
  .map(([path, pkg]) => `${path.replace(/.*node_modules\//, "")}@${pkg.version}: ${pkg.license ?? "no licence"}`);

if (rejected.length > 0) {
  console.error(`Packages without a permissive licence:\n  ${rejected.join("\n  ")}`);
  process.exit(1);
}
console.log(`${Object.keys(lock.packages).length - 1} packages, all under permissive licences.`);
