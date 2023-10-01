#!/usr/bin/env bash

CONTAINER_NAME="rockylinux8-postgres15"

docker ps | grep ${CONTAINER_NAME}
if [ $? != 0 ]; then
  docker run -it -d --name ${CONTAINER_NAME} ${CONTAINER_NAME}-image
fi

# Launch a shell for the running docker container
docker exec -it ${CONTAINER_NAME} /bin/bash
