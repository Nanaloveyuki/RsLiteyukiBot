#!/usr/bin/env sh

set -eu

if [ "$#" -ne 1 ]; then
  echo "Usage: ./scripts/publish_new_version.sh <version>"
  echo "Example: ./scripts/publish_new_version.sh 0.1.0"
  exit 1
fi

RAW_VERSION="$1"
case "$RAW_VERSION" in
  v*) TAG="$RAW_VERSION" ;;
  *) TAG="v$RAW_VERSION" ;;
esac

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "Error: current directory is not a git repository."
  exit 1
fi

if ! git remote get-url origin >/dev/null 2>&1; then
  echo "Error: git remote 'origin' is not configured."
  exit 1
fi

if [ -n "$(git status --porcelain)" ]; then
  echo "Error: working tree is not clean. Commit or stash changes first."
  exit 1
fi

if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null 2>&1; then
  echo "Error: local tag '$TAG' already exists."
  exit 1
fi

if [ -n "$(git ls-remote --tags origin "refs/tags/$TAG")" ]; then
  echo "Error: remote tag '$TAG' already exists on origin."
  exit 1
fi

echo "Creating annotated tag $TAG ..."
git tag -a "$TAG" -m "Release $TAG"

echo "Pushing tag $TAG to origin ..."
git push origin "$TAG"

echo "Done."
echo "GitHub Actions will publish the image to ghcr.io/nanaloveyuki/rsliteyukibot-web:$TAG"
