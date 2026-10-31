const [major, minor] = process.versions.node.split('.').map(Number);
if (major < 26 || (major === 26 && minor < 4)) {
  console.error(`OpenTUI source mode requires Node.js 26.4.0 or newer with --experimental-ffi (found ${process.version}).`);
  process.exit(1);
}
