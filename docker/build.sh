#!/bin/sh
# USAGE: ./build.sh <tag>
export DOCKER_BUILDKIT=1

set -e

if [ ! $1 ]
then 
    echo "Error. Tag not specified" 
    exit 1
fi

IMAGE_TAG="$1"
GIT_REPOSITORY=https://github.com/massalabs/massa.git
DIR="$( cd "$( dirname "$0" )" && pwd )"
DOCKERFILE="$DIR/Dockerfile"
BUILD_DATE="$(date -u +'%Y-%m-%d')"

echo
echo "Building massa-node docker image"
echo "Dockerfile: \t$DOCKERFILE"
echo "docker context: $DIR"
echo "build date: \t$BUILD_DATE"
echo "image tag: \t$IMAGE_TAG"
echo

sed -i "s/^TAG=.*$/TAG=${IMAGE_TAG}/" "$DIR/.env"

docker build -f "$DOCKERFILE" "$DIR" \
     --build-arg IMAGE_TAG="$IMAGE_TAG" \
     --build-arg GIT_REPOSITORY="$GIT_REPOSITORY" \
     --build-arg BUILD_DATE="$BUILD_DATE" \
     --tag massa-node:$IMAGE_TAG  