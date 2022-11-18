#!/bin/bash

cargo build --release
cp ./target/release/relayer-rs ./docker

DOCKER_TAG=$(date +%s)
docker build --tag zkbob-cloud:$DOCKER_TAG ./docker
docker tag zkbob-cloud:$DOCKER_TAG zkbob-cloud:latest

rm ./docker/relayer-rs
