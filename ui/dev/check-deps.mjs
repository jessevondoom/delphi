// Supply-chain gate (D-28): the production dependency tree is small, pinned and script-free.
import { readFileSync } from 'node:fs';
const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
const lock = JSON.parse(readFileSync('package-lock.json', 'utf8'));
const ALLOWED = ['chalk', 'ink', 'marked', 'react', 'string-width', 'wrap-ansi'];
const MAX_PROD_PACKAGES = 60;
const problems = [];

for (const [name, range] of Object.entries(pkg.dependencies ?? {})) {
  if (!ALLOWED.includes(name)) problems.push(`production dependency not on the allowlist: ${name}`);
  if (!/^\d+\.\d+\.\d+$/.test(range)) problems.push(`${name} is not pinned to an exact version: ${range}`);
}
for (const name of ALLOWED) if (!(name in (pkg.dependencies ?? {}))) problems.push(`allowlisted but missing: ${name}`);

const prod = Object.entries(lock.packages).filter(([k, v]) => k && !v.dev && !v.optional);
if (prod.length > MAX_PROD_PACKAGES) problems.push(`production tree has ${prod.length} packages (limit ${MAX_PROD_PACKAGES})`);
for (const [k, v] of prod) {
  if (v.hasInstallScript) problems.push(`install script in production dependency: ${k}`);
  if (!v.integrity) problems.push(`no integrity hash: ${k}`);
  if (v.resolved && !v.resolved.startsWith('https://registry.npmjs.org/')) problems.push(`not from the npm registry: ${k}`);
}
if (problems.length) { console.error(problems.join('\n')); process.exit(1); }
console.log(`ok: ${prod.length} production packages, all pinned, hashed, script-free`);
