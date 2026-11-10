// Assembles the npm packages of a release from the binaries CI built.
//
//   node npm/build.mjs <version> <binaries> <out>
//
// <binaries> holds one folder per Rust target, `obmem-<target>/obmem[.exe]`, as
// actions/download-artifact leaves them. <out> gets one package per platform, holding only the
// binary, and the main package, which depends on all of them as optional dependencies and runs
// the one npm installed (npm/obmem/bin/obmem.js). The main package's name is the one in
// npm/obmem/package.json, scope included, and a platform package is `<that name>-<platform>-<arch>`.
// It prints the folders in the order to publish them: the main package last, so that it never
// points at a platform package that is not there yet.

import { chmodSync, cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..");
const [version, binaries, out] = process.argv.slice(2);
if (!version || !binaries || !out) {
  console.error("usage: node npm/build.mjs <version> <binaries> <out>");
  process.exit(2);
}

const cargoVersion = /\[workspace\.package\][^[]*?^version = "([^"]+)"/ms.exec(
  readFileSync(join(root, "Cargo.toml"), "utf8"),
)?.[1];
if (version !== cargoVersion) {
  console.error(`the release is ${version}, but Cargo.toml builds ${cargoVersion}: they must be the same`);
  process.exit(1);
}

const targets = JSON.parse(readFileSync(join(here, "obmem/targets.json"), "utf8"));
const main = JSON.parse(readFileSync(join(here, "obmem/package.json"), "utf8"));
const writeJson = (file, value) => writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);

rmSync(out, { recursive: true, force: true });
const folders = [];
for (const [platform, target] of Object.entries(targets)) {
  const [os, cpu] = platform.split("-");
  const file = os === "win32" ? "obmem.exe" : "obmem";
  const binary = join(binaries, `obmem-${target}`, file);
  if (!existsSync(binary)) {
    console.error(`no binary for ${target}: ${binary} is missing`);
    process.exit(1);
  }
  const name = `${main.name}-${platform}`;
  const folder = join(out, `obmem-${platform}`);
  mkdirSync(folder, { recursive: true });
  cpSync(binary, join(folder, file));
  // Artifacts do not keep the executable bit.
  chmodSync(join(folder, file), 0o755);
  cpSync(join(root, "LICENSE"), join(folder, "LICENSE"));
  writeFileSync(
    join(folder, "README.md"),
    `# ${name}\n\nThe obmem binary for ${platform}. Install [${main.name}](https://www.npmjs.com/package/${main.name}) instead: it picks the binary for your machine.\n`,
  );
  writeJson(join(folder, "package.json"), {
    name,
    version,
    description: `The obmem binary for ${platform}`,
    license: main.license,
    repository: main.repository,
    os: [os],
    cpu: [cpu],
    files: [file],
  });
  folders.push(folder);
}

const folder = join(out, "obmem");
cpSync(join(here, "obmem"), folder, { recursive: true });
cpSync(join(root, "LICENSE"), join(folder, "LICENSE"));
cpSync(join(root, "README.md"), join(folder, "README.md"));
writeJson(join(folder, "package.json"), {
  ...main,
  version,
  optionalDependencies: Object.fromEntries(
    Object.keys(targets).map((platform) => [`${main.name}-${platform}`, version]),
  ),
});
folders.push(folder);

console.log(folders.join("\n"));
