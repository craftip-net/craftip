#!/bin/bash
set -e

cd "$(dirname "$0")"

if [ -z "$1" ]; then
    echo "Error: no bundle.zip provided!"
    exit 1
fi

if [ -z "$CRAFTIP_SERVER" ]; then
    echo "Error: No \$CRAFTIP_SERVER provided. <username>@<server>"
    exit 1
fi

if [ -z "$CRAFTIP_SERVER_DIR" ]; then
    echo "Error: No \$CRAFTIP_SERVER_DIR provided. /var/www/update.craftip.net"
    exit 1
fi



BUNDLE="/tmp/craftip-bundle"
rm -fr /tmp/craftip-bundle
mkdir /tmp/craftip-bundle

unzip $1 -d $BUNDLE/input

cargo run -- --input $BUNDLE/input/bin --output $BUNDLE/output --ver `cat ${BUNDLE}/input/version`
scp -r $BUNDLE/output/* "${CRAFTIP_SERVER}:${CRAFTIP_SERVER_DIR}/update/v1/"
cargo run -- --test-staging
scp -r $BUNDLE/input/downloads/ "${CRAFTIP_SERVER}:${CRAFTIP_SERVER_DIR}/downloads_staging"
ssh ${CRAFTIP_SERVER} "\
    mv ${CRAFTIP_SERVER_DIR}/update/v1/latest.json.staging.json ${CRAFTIP_SERVER_DIR}/update/v1/latest.json;\
    rm -rf ${CRAFTIP_SERVER_DIR}/downloads;\
    mv ${CRAFTIP_SERVER_DIR}/downloads_staging ${CRAFTIP_SERVER_DIR}/downloads"