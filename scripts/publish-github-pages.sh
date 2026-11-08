#!/usr/bin/env bash

set -euo pipefail

remote="${1:-origin}"
branch="${2:-gh-pages}"
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd -- "$script_dir/.." && pwd -P)"
web_root="$repo_root/apps/web"
dist_root="$web_root/dist"

if [[ -n "$(git -C "$repo_root" status --porcelain)" ]]; then
    echo "Commit or stash local changes before publishing." >&2
    exit 1
fi

(
    cd -- "$web_root"
    npm run build
    npm run test:wasm
)

if [[ -n "$(git -C "$repo_root" status --porcelain)" ]]; then
    echo "The build changed tracked files. Review and commit them before publishing." >&2
    exit 1
fi

if [[ ! -f "$dist_root/index.html" ]]; then
    echo "The build did not produce apps/web/dist/index.html." >&2
    exit 1
fi

remote_url="$(git -C "$repo_root" remote get-url "$remote")"
remote_head="$(git -C "$repo_root" ls-remote --heads "$remote" "$branch")"

temp_root="$(cd -- "${TMPDIR:-/tmp}" && pwd -P)"
publish_root="$(mktemp -d "$temp_root/fastchart-pages.XXXXXX")"

cleanup() {
    case "$publish_root" in
        "$temp_root"/fastchart-pages.*)
            rm -rf -- "$publish_root"
            ;;
        *)
            echo "Refusing to remove unexpected path: $publish_root" >&2
            ;;
    esac
}
trap cleanup EXIT

git -C "$publish_root" init --initial-branch="$branch"
git -C "$publish_root" remote add origin "$remote_url"

if [[ -n "$remote_head" ]]; then
    git -C "$publish_root" fetch --depth=1 origin "$branch"
    git -C "$publish_root" checkout -B "$branch" FETCH_HEAD
fi

git -C "$publish_root" rm -r --ignore-unmatch . >/dev/null
git -C "$publish_root" clean -fdx >/dev/null
cp -R "$dist_root"/. "$publish_root"/
touch "$publish_root/.nojekyll"

author_name="$(git -C "$repo_root" config user.name || true)"
author_email="$(git -C "$repo_root" config user.email || true)"
if [[ -z "$author_name" || -z "$author_email" ]]; then
    echo "Configure Git user.name and user.email before publishing." >&2
    exit 1
fi

git -C "$publish_root" config user.name "$author_name"
git -C "$publish_root" config user.email "$author_email"
git -C "$publish_root" config core.autocrlf false
git -C "$publish_root" add --all

if [[ -z "$(git -C "$publish_root" status --porcelain)" ]]; then
    echo "GitHub Pages is already up to date."
    exit 0
fi

source_commit="$(git -C "$repo_root" rev-parse --short HEAD)"
git -C "$publish_root" commit -m "Deploy FastChart $source_commit"
git -C "$publish_root" push origin "HEAD:refs/heads/$branch"
echo "Published FastChart to $remote/$branch."
