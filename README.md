# CraftIP - A tunnel for Minecraft servers

This is the complete source code of CraftIP. Feel free to contribute!

## Architecture

The project is split into 3 parts:
1. `server` This is the proxy server terminating the Minecraft client connections and forwarding the traffic to the CraftIP client.
2. `client` The networking logic of the client of CraftIP. This forwards the connections from the CraftIP server to the Minecraft server.
3. `client-gui`: The GUI for CraftIP, making CraftIP more accessible for users.

## Compile

The software is written in Rust. To compile it, you have to [install](https://www.rust-lang.org/tools/install) the Rust compiler chain.

### Compile GUI
If you want to build CraftIP yourself, please run:

```bash
cargo run --release --bin client-gui
```

### Running on a Raspberry PI
You can also run this software on a Raspberry PI. To save resources, you can use the command line version (`client`).

On Raspbian, you have to install the Rust compiler toolchain as well as Git:
```bash
sudo apt-get install -y cargo git
```
Download the repo:
```bash
git clone https://github.com/craftip-net/craftip.git
cd craftip
```
After starting the Minecraft server on the standard port `25565` you can compile and run the client:
```bash
cargo run --release --bin client
```

Happy Crafting :)

