#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_dir="${repo_root}/site"
coverage_dir="${repo_root}/coverage/html"
output_dir="${repo_root}/public"

if [[ ! -f "${coverage_dir}/index.html" ]]; then
  echo "coverage/html/index.html is missing. Run 'just coverage-html' first." >&2
  exit 1
fi

rm -rf "${output_dir}"
mkdir -p "${output_dir}/assets" "${output_dir}/coverage"

cp -R "${source_dir}/." "${output_dir}/"
cp -R "${repo_root}/assets/." "${output_dir}/assets/"
cp -R "${coverage_dir}/." "${output_dir}/coverage/"

touch "${output_dir}/.nojekyll"
