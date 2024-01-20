#!/bin/bash

echo "Building magic Docker container!"
#docker build -t dscso/rust-crosscompiler:latest https://github.com/dscso/rust-crosscompiler.git#main
#docker pull dscso/rust-crosscompiler:latest
echo "Stopping old Dockercontainer"
docker stop crosscompiler
echo "Removing old Dockercontainer"
docker rm crosscompiler
echo "Starting new Dockercontainer, with volumes..."
docker run -v $(pwd)/target-cross:/build/target                               \
           -v $(pwd)/Cargo.toml:/build/Cargo.toml:ro                          \
           -v $(pwd)/shared:/build/shared:ro                                  \
           -v $(pwd)/client:/build/client:ro                                  \
           -v $(pwd)/client-gui:/build/client-gui:ro                          \
           -v $(pwd)/server:/build/server:ro                                  \
           --name crosscompiler    -d                                         \
           dscso/rust-crosscompiler:latest                                    \
           sleep infinity

export cargocommand="source /entrypoint.sh && cd client-gui; cargo build --config /root/.cargo/config"

if [ "$1" == "release" ]; then
  echo "Building release version!"
  export cargocommand="$cargocommand --release"
fi
echo $cargocommand
export runincontainer="docker exec -it crosscompiler "

echo "Container running... Building application... x86_64-pc-windows-gnu"
$runincontainer /bin/bash -c "$cargocommand --target=x86_64-pc-windows-gnu"

echo "Container running... Building application... x86_64-apple-darwin"
$runincontainer /bin/bash -c "$cargocommand --target=x86_64-apple-darwin"

echo "Container running... Building application... aarch64-apple-darwin"
$runincontainer /bin/bash -c "$cargocommand --target=aarch64-apple-darwin"


docker stop crosscompiler
docker rm crosscompiler
