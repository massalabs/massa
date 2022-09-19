#!/bin/sh
# USAGE: ./build.sh <tag>
set -e

DIR="$( cd "$( dirname "$0" )" && pwd )"
DOCKERFILE="$DIR/Dockerfile"
GIT_COMMIT="$1"
BUILD_DATE="$(date -u +'%Y-%m-%d')"
IMAGE_TAG="$GIT_COMMIT"

echo
echo "Building massa-node docker image"
echo "Dockerfile: \t$DOCKERFILE"
echo "docker context: $DIR"
echo "build date: \t$BUILD_DATE"
echo "git commit: \t$GIT_COMMIT"
echo "image tag: \t$IMAGE_TAG"
echo

docker build -f "$DOCKERFILE" "$DIR" \
        --build-arg GIT_COMMIT="$GIT_COMMIT" \
        --build-arg BUILD_DATE="$BUILD_DATE" \
        --tag massa-node:$IMAGE_TAG  