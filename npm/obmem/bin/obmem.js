#!/usr/bin/env node
// Runs the obmem binary for this machine. npm installed it as the one optional dependency
// whose `os` and `cpu` fit (see ../targets.json and npm/build.mjs in the repository).
"use strict";

const { spawnSync } = require("node:child_process");
const targets = require("../targets.json");

const platform = `${process.platform}-${process.arch}`;
if (!(platform in targets)) {
  console.error(
    `obmem: there is no prebuilt binary for ${platform}. Build it from source: https://github.com/Chia1104/obmem`,
  );
  process.exit(1);
}

let binary;
try {
  binary = require.resolve(
    `obmem-${platform}/${process.platform === "win32" ? "obmem.exe" : "obmem"}`,
  );
} catch {
  console.error(
    `obmem: the package obmem-${platform} is missing. npm leaves it out when optional dependencies are off (--omit=optional): install again without that.`,
  );
  process.exit(1);
}

// The binary takes over stdin and stdout: as an MCP server and in agent hooks they carry data.
const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(`obmem: could not run ${binary}: ${result.error.message}`);
  process.exit(1);
}
if (result.signal) {
  process.kill(process.pid, result.signal);
}
process.exit(result.status ?? 1);
