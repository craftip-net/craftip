#!/bin/bash
set -e

cd "$(dirname "$0")"

if [ -z "$1" ]; then
    echo "Error: no bundle.zip provided!"
    exit 1
fi

if [ -z "$CRAFTIP_SERVER" ]; then
    echo "Error: No \$CRAFTIP_SERVER provided."
    exit 1
fi

if [ -z "$CRAFTIP_SERVER_DIR" ]; then
    echo "Error: No \$CRAFTIP_SERVER_DIR provided."
    exit 1
fi



BUNDLE="/tmp/craftip-bundle"
rm -fr /tmp/craftip-bundle
mkdir /tmp/craftip-bundle

unzip $1 -d $BUNDLE/input

cargo run -- --input $BUNDLE/input/bin --output $BUNDLE/output --ver `cat ${BUNDLE}/input/version`
scp -r $BUNDLE/output/* "${CRAFTIP_SERVER}:${CRAFTIP_SERVER_DIR}/"
cargo run -- --test-staging
ssh ${CRAFTIP_SERVER} "mv ${CRAFTIP_SERVER_DIR}/latest.json.staging.json ${CRAFTIP_SERVER_DIR}/latest.json"