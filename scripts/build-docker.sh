#!/bin/bash

DOCKER_TAG=$(date +%s)
docker build --file ./docker/Dockerfile --tag zkbob-cloud:$DOCKER_TAG .
docker tag zkbob-cloud:$DOCKER_TAG zkbob-cloud:latest
