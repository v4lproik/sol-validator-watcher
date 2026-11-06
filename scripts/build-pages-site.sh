#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_dir="${repo_root}/site"
coverage_dir="${repo_root}/coverage/html"
output_dir="${repo_root}/public"

fingerprint_asset() {
  local relative_path="$1"
  local source_path="${output_dir}/${relative_path}"
  local directory filename stem extension digest fingerprinted_filename fingerprinted_path

  directory="${relative_path%/*}"
  filename="${relative_path##*/}"
  stem="${filename%.*}"
  extension="${filename##*.}"
  digest="$(git -C "${repo_root}" hash-object "${source_path}")"
  fingerprinted_filename="${stem}.${digest:0:12}.${extension}"

  if [[ "${directory}" == "${relative_path}" ]]; then
    fingerprinted_path="${fingerprinted_filename}"
  else
    fingerprinted_path="${directory}/${fingerprinted_filename}"
  fi

  mv "${source_path}" "${output_dir}/${fingerprinted_path}"
  printf '%s' "${fingerprinted_path}"
}

replace_asset_reference() {
  local page_path="$1"
  local source_reference="$2"
  local fingerprinted_reference="$3"
  local page

  page="$(<"${page_path}")"
  if [[ "${page}" != *"${source_reference}"* ]]; then
    echo "${page_path} does not reference ${source_reference}" >&2
    exit 1
  fi

  page="${page//${source_reference}/${fingerprinted_reference}}"
  printf '%s\n' "${page}" > "${page_path}"
}

if [[ ! -f "${coverage_dir}/index.html" ]]; then
  echo "coverage/html/index.html is missing. Run 'just coverage html' first." >&2
  exit 1
fi

rm -rf "${output_dir}"
mkdir -p "${output_dir}/assets" "${output_dir}/coverage"

cp -R "${source_dir}/." "${output_dir}/"
cp -R "${repo_root}/assets/." "${output_dir}/assets/"
cp -R "${coverage_dir}/." "${output_dir}/coverage/"

styles_asset="$(fingerprint_asset "styles.css")"
app_asset="$(fingerprint_asset "app.js")"

replace_asset_reference "${output_dir}/index.html" "styles.css" "${styles_asset}"
replace_asset_reference "${output_dir}/404.html" "styles.css" "${styles_asset}"
replace_asset_reference "${output_dir}/index.html" "app.js" "${app_asset}"
replace_asset_reference "${output_dir}/404.html" "app.js" "${app_asset}"

touch "${output_dir}/.nojekyll"
