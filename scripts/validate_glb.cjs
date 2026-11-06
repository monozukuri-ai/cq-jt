// Khronos glTF-Validator is a development tool, never a package dependency.
// Usage: node scripts/validate_glb.cjs <validator-module-path> <report.json> <file.glb>...
const fs = require('fs');
const path = require('path');
const [modulePath, reportPath, ...files] = process.argv.slice(2);
if (!modulePath || !reportPath || !files.length) throw Error('expected validator module, report, GLB files');
const validator = require(path.resolve(modulePath));
(async () => {
  const results = [];
  for (const file of files) {
    const report = await validator.validateBytes(new Uint8Array(fs.readFileSync(file)), {
      uri: path.basename(file), maxIssues: 10000,
      externalResourceFunction: async () => { throw Error('external resources are not expected in a self-contained GLB'); }
    });
    results.push({file, ...report});
    process.stdout.write(`${path.basename(file)}: ${report.issues.numErrors} errors, ${report.issues.numWarnings} warnings\n`);
  }
  fs.writeFileSync(reportPath, JSON.stringify({validatorVersion: validator.version(), results}, null, 2) + '\n');
  if (results.some(r => r.issues.numErrors || r.issues.numWarnings || r.issues.truncated)) process.exitCode = 1;
})().catch(e => { console.error(e); process.exitCode = 1; });
