#!/usr/bin/env bash
set -euo pipefail

version=v1.7.0
destination=${1:-"$HOME/.local/bin"}
os=$(uname -s)
arch=$(uname -m)

case "${os}/${arch}" in
  Darwin/arm64)
    asset=surfpool-darwin-arm64.tar.gz
    checksum=32c309e3b79bfa3913e384b3c9b109000720cfe7f346d145feb6693fccfb202f
    ;;
  Darwin/x86_64)
    asset=surfpool-darwin-x64.tar.gz
    checksum=b9724b6a67ac33b9cc94fb41adaf47bd638269142c7ddc6a0dae7ca77fd9e760
    ;;
  Linux/x86_64)
    asset=surfpool-linux-x64.tar.gz
    checksum=5bef2b0b186a74622508f104b6d2e4e2db040932b6d8f3a811172e241f802a63
    ;;
  *)
    echo "Surfpool ${version} has no pinned release asset for ${os}/${arch}" >&2
    exit 1
    ;;
esac

for command in curl tar; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "required command is unavailable: $command" >&2
    exit 1
  }
done

test_dir=$(mktemp -d)
cleanup() {
  rm -rf "$test_dir"
}
trap cleanup EXIT
archive="$test_dir/$asset"
url="https://github.com/solana-foundation/surfpool/releases/download/${version}/${asset}"

curl --fail --location --retry 5 --retry-all-errors --retry-delay 2 \
  --output "$archive" "$url"
if command -v sha256sum >/dev/null 2>&1; then
  actual_checksum=$(sha256sum "$archive")
elif command -v shasum >/dev/null 2>&1; then
  actual_checksum=$(shasum -a 256 "$archive")
else
  echo "sha256sum or shasum is required" >&2
  exit 1
fi
actual_checksum=${actual_checksum%% *}
[[ "$actual_checksum" == "$checksum" ]] || {
  echo "Surfpool ${version} checksum mismatch for ${asset}" >&2
  exit 1
}

tar -xzf "$archive" -C "$test_dir"
[[ -x "$test_dir/surfpool" ]] || {
  echo "Surfpool release archive did not contain an executable named surfpool" >&2
  exit 1
}
mkdir -p "$destination"
install -m 0755 "$test_dir/surfpool" "$destination/surfpool"
"$destination/surfpool" --version
