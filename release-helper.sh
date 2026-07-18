#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

if (( $# > 1 )); then
  echo "usage: ./release-helper.sh [version]" >&2
  exit 1
fi

version="${1:-}"
if [[ -z "$version" ]]; then
  printf 'version: '
  read -r version
fi
version="${version#v}"

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
  echo "version must look like 1.2.3 or 1.2.3-beta.1" >&2
  exit 1
fi

tag="v${version}"

for command in git npm cargo; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "$command is required" >&2
    exit 1
  fi
done

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "run this helper from the Koma repository" >&2
  exit 1
fi

if git rev-parse "$tag" >/dev/null 2>&1; then
  echo "tag $tag already exists locally" >&2
  exit 1
fi

if ! remote_tag="$(git ls-remote --tags origin "refs/tags/$tag")"; then
  echo "could not check tags on origin" >&2
  exit 1
fi
if [[ -n "$remote_tag" ]]; then
  echo "tag $tag already exists on origin" >&2
  exit 1
fi

branch="$(git branch --show-current)"
if [[ -z "$branch" ]]; then
  echo "releases cannot be created from a detached HEAD" >&2
  exit 1
fi

if ! git rev-parse --abbrev-ref '@{upstream}' >/dev/null 2>&1; then
  echo "branch $branch does not have an upstream" >&2
  exit 1
fi

echo
echo "Koma $tag"
echo "Branch: $branch"
echo
if [[ -n "$(git status --short)" ]]; then
  echo "Changes included in this release:"
  git status --short
else
  echo "The current commit will be tagged without a new release commit."
fi
echo

if [[ "${KOMA_RELEASE_YES:-0}" != "1" ]]; then
  printf 'Continue? [y/N] '
  read -r confirmation
  if [[ ! "$confirmation" =~ ^[Yy]$ ]]; then
    echo "Release cancelled."
    exit 0
  fi
fi

npm run release:version -- "$version"
npm run release:check
npm run typecheck
npm run i18n:check
npm run lint
npm run test:run
cargo check --workspace --all-targets

git add --all
if ! git diff --cached --quiet; then
  git commit -m "release $tag"
fi

git push
git tag -a "$tag" -m "Koma $tag"
git push origin "$tag"

echo
echo "$tag pushed. GitHub Actions will create the draft release and attach the installers."
