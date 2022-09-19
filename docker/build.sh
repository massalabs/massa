#!/bin/sh
set -e

DIR="$( cd "$( dirname "$0" )" && pwd )"
REPO_ROOT="$(git rev-parse --show-toplevel)"
DOCKERFILE="$DIR/Dockerfile"
GIT_COMMIT="$(git log --format="%H" -n 1)"
BUILD_DATE="$(date -u +'%Y-%m-%d')"
IMAGE_TAG="$(git tag | sort -V | tail -1)"

echo
echo "Building massa-node docker image"
echo "Dockerfile: \t$DOCKERFILE"
echo "docker context: $REPO_ROOT"
echo "build date: \t$BUILD_DATE"
echo "git commit: \t$GIT_COMMIT"
echo "image tag: \t$IMAGE_TAG"
echo

docker build -f "$DOCKERFILE" "$REPO_ROOT" \
	--build-arg GIT_REVISION="$GIT_REVISION" \
	--build-arg BUILD_DATE="$BUILD_DATE" \
	--tag massa-node:$IMAGE_TAG "$@"
