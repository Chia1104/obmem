# obmem-update

Finds out how the running `obmem` was installed and what the newest version is, and updates an install that nothing else manages. Like obmem-sync, it holds network code behind a blocking API.

## Who updates what (`Install`)

The same binary reaches a machine in several ways, and whoever put it there owns the file. `Install::of` decides from the binary's own path:
- `Npm`: inside a `node_modules` folder. npm keeps its own record of the version, so obmem only prints the npm command.
- `Source`: in cargo's bin folder or under a `target` folder. Built from a clone: obmem prints the cargo command.
- `Standalone`: anywhere else, which is where `scripts/install.sh` and `scripts/install.ps1` put it. Only this one obmem replaces itself.

A new way to install adds a variant here. Nothing is recorded at install time: the path is enough.

## The newest version (`latest`)

- An npm install asks npm for the package's `latest` tag. Everything else asks GitHub which release is the latest. A release is on GitHub minutes before npm serves it, so asking GitHub for an npm install would announce a version that cannot be installed yet.
- `NPM_PACKAGE` repeats the name in `npm/obmem/package.json`; a test compares the two. The repository comes from `CARGO_PKG_REPOSITORY`.

## Replacing a standalone binary (`install`)

`install` downloads the install script that the new release carries and runs it with `OBMEM_VERSION` and `OBMEM_INSTALL_DIR` set to the binary's folder. The script picks the archive, checks it against `SHA256SUMS` and swaps the file, so there is one implementation of installing, and the release workflow's `verify` job tests it on every platform.
- The contract with past versions is small: every release has `install.sh` and `install.ps1`, and they honour those two variables. An old binary can then update to any later release, whatever else changes.
- Both platforms take the same code path, a script in a temp file run by `sh` or `powershell`. Only the names differ, so the Windows side is type-checked on every build.

## Rules

- Nothing here runs in a hook or at start-up. An update check is a network call, and it happens only when the user asks.
- No update is installed without the user asking: `obmem update --check` and `obmem status` only report.
