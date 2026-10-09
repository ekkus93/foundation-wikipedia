#!/usr/bin/env node
/**
 * Fail-closed SPDX license review for the desktop's installed npm graph.
 * npm's node_modules layout is traversed explicitly, including nested packages.
 * This is a license gate, not a vulnerability scanner or a complete SBOM.
 */
import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

export const APPROVED_LICENSES = new Set([
  "MIT", "ISC", "BSD-2-Clause", "BSD-3-Clause", "Apache-2.0",
  "Apache-2.0 OR MIT", "MIT OR Apache-2.0", "0BSD", "CC0-1.0",
  "MPL-2.0", "Unlicense", "BlueOak-1.0.0", "CC-BY-4.0",
]);

export function licenseError(pkg) {
  if (!pkg || typeof pkg.name !== "string" || typeof pkg.version !== "string") {
    return "Malformed npm dependency package metadata";
  }
  if (typeof pkg.license !== "string" || !APPROVED_LICENSES.has(pkg.license)) {
    return `${pkg.name}@${pkg.version}: unapproved or missing license ${JSON.stringify(pkg.license)}`;
  }
  return null;
}

async function directories(folder) {
  const entries = await readdir(folder, { withFileTypes: true });
  return entries.filter((entry) => entry.isDirectory() && !entry.name.startsWith("."));
}

export async function auditInstalledPackages(folder) {
  const errors = [];
  let scanned = 0;

  async function scanModuleDirectory(nodeModules) {
    for (const entry of await directories(nodeModules)) {
      if (entry.name.startsWith("@")) {
        for (const scoped of await directories(join(nodeModules, entry.name))) {
          await scanPackage(join(nodeModules, entry.name, scoped.name));
        }
      } else {
        await scanPackage(join(nodeModules, entry.name));
      }
    }
  }

  async function scanPackage(directory) {
    const pkg = JSON.parse(await readFile(join(directory, "package.json"), "utf8"));
    scanned += 1;
    const error = licenseError(pkg);
    if (error) errors.push(error);
    const nested = join(directory, "node_modules");
    try {
      await scanModuleDirectory(nested);
    } catch (error) {
      if (error?.code !== "ENOENT") throw error;
    }
  }

  await scanModuleDirectory(folder);
  if (scanned === 0) errors.push("No installed npm dependencies to audit");
  return { scanned, errors };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const { scanned, errors } = await auditInstalledPackages(join(process.cwd(), "node_modules"));
  for (const error of errors) console.error(error);
  if (errors.length) process.exitCode = 1;
  else console.log(`Desktop dependency licenses verified: ${scanned} packages`);
}
