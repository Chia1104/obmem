#!/bin/sh
# Installs the obmem binary of a GitHub release on macOS or Linux.
#
#   curl -fsSL https://github.com/Chia1104/obmem/releases/latest/download/install.sh | sh
#
# It downloads the archive for this machine, checks it against the release's SHA256SUMS and
# puts `obmem` in ~/.obmem/bin. Nothing else on the machine is changed: when that folder is not
# on PATH, it says what to add.
#
#   OBMEM_VERSION       the release to install, such as 0.1.0 (default: the latest)
#   OBMEM_INSTALL_DIR   where the binary goes (default: ~/.obmem/bin)
#   OBMEM_RELEASE_URL   the folder that holds a release's files, for a mirror or a test
#                       (default: that release on GitHub)
set -eu

REPOSITORY="Chia1104/obmem"

fail() {
  echo "obmem install: $*" >&2
  exit 1
}

# The Rust target of the release archive for this machine.
target() {
  case "$(uname -m)" in
    x86_64 | amd64) arch=x86_64 ;;
    arm64 | aarch64) arch=aarch64 ;;
    *) fail "there is no prebuilt binary for the $(uname -m) architecture" ;;
  esac
  case "$(uname -s)" in
    Darwin)
      # A shell that runs under Rosetta reports x86_64 on an Apple silicon Mac.
      if [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || true)" = "1" ]; then
        arch=aarch64
      fi
      echo "$arch-apple-darwin"
      ;;
    Linux) echo "$arch-unknown-linux-musl" ;;
    *) fail "there is no prebuilt binary for $(uname -s); on Windows use install.ps1" ;;
  esac
}

download() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$1" -O "$2"
  else
    fail "curl or wget is needed to download obmem"
  fi || fail "could not download $1"
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d ' ' -f 1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d ' ' -f 1
  else
    fail "sha256sum or shasum is needed to check the download"
  fi
}

main() {
  version="${OBMEM_VERSION:-latest}"
  version="${version#v}"
  if [ -n "${OBMEM_RELEASE_URL:-}" ]; then
    url="$OBMEM_RELEASE_URL"
  elif [ "$version" = latest ]; then
    url="https://github.com/$REPOSITORY/releases/latest/download"
  else
    url="https://github.com/$REPOSITORY/releases/download/v$version"
  fi
  dir="${OBMEM_INSTALL_DIR:-$HOME/.obmem/bin}"
  archive="obmem-$(target).tar.gz"

  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  download "$url/$archive" "$tmp/$archive"
  download "$url/SHA256SUMS" "$tmp/SHA256SUMS"

  expected="$(awk -v name="$archive" '$2 == name || $2 == "*" name { print $1 }' "$tmp/SHA256SUMS")"
  [ -n "$expected" ] || fail "SHA256SUMS of this release does not list $archive"
  actual="$(sha256 "$tmp/$archive")"
  [ "$actual" = "$expected" ] || fail "$archive does not match its checksum (expected $expected, got $actual)"

  tar -xzf "$tmp/$archive" -C "$tmp"
  mkdir -p "$dir"
  # Into the folder under another name first, then renamed: an `obmem` that is running keeps its
  # old file, and nothing ever finds a half-written one.
  cp "$tmp/obmem" "$dir/.obmem.new"
  chmod 755 "$dir/.obmem.new"
  mv -f "$dir/.obmem.new" "$dir/obmem"

  echo "installed $("$dir/obmem" --version) to $dir/obmem"
  found="$(command -v obmem 2>/dev/null || true)"
  if [ -z "$found" ]; then
    echo "$dir is not on PATH. Add it in your shell's startup file:"
    echo "  export PATH=\"$dir:\$PATH\""
  elif [ "$found" != "$dir/obmem" ]; then
    echo "note: \`obmem\` on PATH is $found, another install that comes before $dir"
  fi
}

# Run only once the whole script has arrived: a download that stops halfway does nothing.
main "$@"
