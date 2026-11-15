// Supply-chain gate (spec 111 §6): the tree that ships in the window is small, pinned, hashed and script-free.
import { readFileSync } from 'node:fs';
const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
const lock = JSON.parse(readFileSync('package-lock.json', 'utf8'));
// `@mermaid-js/tiny` is Mermaid built into one file with no dependencies, for the diagrams of spec 118 §5 item 4. It must
// stay that way: a dependency of its own fails the check below.
const ALLOWED = ['@tauri-apps/api', 'react', 'react-dom', '@mermaid-js/tiny'];
const NO_DEPENDENCIES = ['@mermaid-js/tiny'];
const MAX_PROD_PACKAGES = 10;
const problems = [];

for (const group of ['dependencies', 'devDependencies']) {
  for (const [name, range] of Object.entries(pkg[group] ?? {})) {
    if (!/^\d+\.\d+\.\d+$/.test(range)) problems.push(`${name} is not pinned to an exact version: ${range}`);
  }
}
for (const name of Object.keys(pkg.dependencies ?? {})) if (!ALLOWED.includes(name)) problems.push(`production dependency not on the allowlist: ${name}`);
for (const name of ALLOWED) if (!(name in (pkg.dependencies ?? {}))) problems.push(`allowlisted but missing: ${name}`);
for (const name of NO_DEPENDENCIES) {
  const entry = lock.packages[`node_modules/${name}`] ?? {};
  const own = { ...entry.dependencies, ...entry.optionalDependencies, ...entry.peerDependencies };
  if (Object.keys(own).length) problems.push(`${name} must have no dependencies of its own: ${Object.keys(own).join(', ')}`);
}

const prod = Object.entries(lock.packages).filter(([k, v]) => k && !v.dev && !v.optional);
if (prod.length > MAX_PROD_PACKAGES) problems.push(`production tree has ${prod.length} packages (limit ${MAX_PROD_PACKAGES})`);
for (const [k, v] of Object.entries(lock.packages)) {
  if (!k) continue;
  if (!v.dev && !v.optional && v.hasInstallScript) problems.push(`install script in production dependency: ${k}`);
  if (!v.integrity) problems.push(`no integrity hash: ${k}`);
  if (v.resolved && !v.resolved.startsWith('https://registry.npmjs.org/')) problems.push(`not from the npm registry: ${k}`);
}
if (problems.length) { console.error(problems.join('\n')); process.exit(1); }
console.log(`ok: ${prod.length} production packages, every version pinned and hashed, none with an install script`);
