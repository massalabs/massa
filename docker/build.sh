#!/bin/bash
export DOCKER_BUILDKIT=1

set -e

GIT_REPOSITORY=https://github.com/massalabs/massa.git
DIR="$( cd "$( dirname "$0" )" && pwd )"
DOCKERFILE="$DIR/Dockerfile"
BUILD_DATE="$(date -u +'%Y-%m-%d')"

read -p "Enter release tag: " -r IMAGE_TAG
read -p "Enter image name: " -r IMAGE_NAME
read -p "Do you want to send the image to DockerHub [yes/no]: " -r PUSH_FLAG

while [[ "$PUSH_FLAG" != "yes" && "$PUSH_FLAG" != "no" ]]
do
    read -r -p "Please answer yes/no: " PUSH_FLAG
done

if [[ "$PUSH_FLAG" == "yes" ]]
then
    read -r -p "Enter DockerHub repository: " DOCKERHUB_REPO
    IMAGE_NAME=$DOCKERHUB_REPO/$IMAGE_NAME:$IMAGE_TAG
    read -r -p "Enter username: " DOCKERHUB_USERNAME
    read -r -p "Enter password: " DOCKERHUB_PASSWORD
else
    IMAGE_NAME=$IMAGE_NAME:$IMAGE_TAG
fi

echo -e "\nBuilding massa-node docker image"
echo -e "Build date: \t$BUILD_DATE"
echo -e "Dockerfile: \t$DOCKERFILE"
echo -e "Docker context: $DIR"
echo -e "Docker Image: \t$IMAGE_NAME"
echo -e "Version: \t$IMAGE_TAG\n"

if [[ "$(uname)" == "Darwin" ]]
then
    sed -i "" "s/^TAG=.*$/TAG=${IMAGE_TAG}/" "$DIR/.env"
else
    sed -i "s/^TAG=.*$/TAG=${IMAGE_TAG}/" "$DIR/.env"
fi

docker build -f "$DOCKERFILE" "$DIR" \
     --build-arg IMAGE_TAG="$IMAGE_TAG" \
     --build-arg GIT_REPOSITORY="$GIT_REPOSITORY" \
     --build-arg BUILD_DATE="$BUILD_DATE" \
     --tag $IMAGE_NAME

if [[ "$PUSH_FLAG" == "yes" ]]
then
    echo -e "\nSending docker image \"$IMAGE_NAME\" to DockerHub\n"
    docker login -u $DOCKERHUB_USERNAME -p $DOCKERHUB_PASSWORD 2>/dev/null
    docker push $IMAGE_NAME
fi

echo -e "\nThe build is complete\n"