#!/usr/bin/env bash

CONTAINER_NAME="rockylinux8-postgres15"

# Checks if the container already runs or not
docker ps | grep ${CONTAINER_NAME} > /dev/null 2>&1
if [ $? != 0 ]; then
  # Run a container from the custom docker image
  docker run -it -d -p 80:80 -p 5432:5432 --user docker --name ${CONTAINER_NAME} \
    ${CONTAINER_NAME}-image
else
  echo "Already running ${CONTAINER_NAME}"...
fi
