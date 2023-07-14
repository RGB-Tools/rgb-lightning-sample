# RGB Lightning Sample

RGB-enabled LN node based on [ldk-sample].

Please notice that an RGB-enabled LN node daemon is available in the
[rgb-lightning-node] repository. For any application interested in integrating
RGB on lightning it is recommended to use the node as this sample has less
features and is less maintained.

The node enables the possibility to create payment channels containing assets
issued using the RGB protocol, as well as routing RGB asset denominated
payments across multiple channels, given that they all possess the necessary
liquidity. In this way, RGB assets can be transferred with the same user
experience and security assumptions of regular Bitcoin Lightning Network
payments. This is achieved by adding to each lightning commitment transaction a
dedicated extra output containing the anchor to the RGB state transition.

More context on how RGB works on the Lightning Network can be found
[here](https://docs.rgb.info/lightning-network-compatibility).

The RGB functionality for now can be tested only in regtest or testnet
environments, but an advanced user may be able to apply changes in order to use
it also on other networks.
Please be careful, this software is early alpha, we do not take any
responsability for loss of funds or any other issue you may encounter.

Also note that the following RGB projects (included in this project as git
sumbodules) have been modified in order to make the creation of static
consignments (without entropy) possible. Here links to compare the applied
changes:
- [client_side_validation](https://github.com/RGB-Tools/client_side_validation/compare/v0.10.4...static_0.10)
- [rgb-wallet](https://github.com/RGB-Tools/rgb-wallet/compare/v0.10.3...static_0.10)

But most importantly [rust-lightning] has been changed in order to support
RGB channels,
[here](https://github.com/RGB-Tools/rust-lightning/compare/v0.0.115...rgb)
a compare with `v0.0.115`, the version we applied the changes to.

## Installation

Clone the project, including (shallow) submodules:
```sh
git clone https://github.com/RGB-Tools/rgb-lightning-sample --recurse-submodules --shallow-submodules
```

Build the ldk-sample crate:
```sh
cargo build
```

## Usage in a test environment

A test environment has been added for easier testing. It currently supports the
regtest and testnet networks.

Instructions and commands are meant to be run from the project's root
directory.

The `docker-compose.yml` file manages, when using the regtest network:
- a bitcoind node
- an electrs instance
- an [RGB proxy server] instance

Run this command in order to start with a clean test environment (specifying
the desired network):
```sh
tests/test.sh --start --network <network>
```

The command will create the directories needed by the services, start the
docker containers and mine some blocks. The test environment will always start
in a clean state, taking down previous running services (if any) and
re-creating data directories.

Once services are running, ldk nodes can be started.
Each ldk node needs to be started in a separate shell with `cargo run`,
specifying:
- bitcoind user, password, host and port
- ldk data directory
- ldk peer listening port
- network

Here's an example of how to start three regtest nodes, each one using the
shared regtest services provided by docker compose:
```sh
# 1st shell
cargo run user:password@localhost:18443 dataldk0/ 9735 regtest

# 2nd shell
cargo run user:password@localhost:18443 dataldk1/ 9736 regtest

# 3rd shell
cargo run user:password@localhost:18443 dataldk2/ 9737 regtest
```

Here's an example of how to start three testnet nodes, each one using the
external testnet services:

```sh
# 1st shell
cargo run user:password@electrum.iriswallet.com:18332 dataldk0/ 9735 testnet

# 2nd shell
cargo run user:password@electrum.iriswallet.com:18332 dataldk1/ 9736 testnet

# 3rd shell
cargo run user:password@electrum.iriswallet.com:18332 dataldk2/ 9737 testnet
```

Once ldk nodes are running, they can be operated via their CLI.
See the [on-chain] and [off-chain] sections below and the CLI `help` command for
information on the available commands.

To stop running nodes, exit their CLI with the `quit` command (or `^D`).

To stop running services and to cleanup data directories, run:
```sh
tests/test.sh --stop
```

If needed, more nodes can be added. To do so:
- add data directories for the additional ldk nodes (`dataldk<n>`)
- run additional `cargo run`s for ldk nodes, specifying the correct bitcoind
  string, data directory, peer listening port and network

## On-chain operations

On-chain RGB operations are available as CLI commands. The following sections
briefly explain how to use each one of them.

### Issuing an asset
To issue a new asset, call the `issueasset` command followed by:
- total supply
- ticker
- name
- precision

Example:
```
issueasset 1000 USDT Tether 0
```

### Receiving assets
To receive assets, call the `receiveasset`.
Provide the sender with the returned blinded UTXO.

Example:
```
receiveasset
```

### Sending assets
To send assets to another node with an on-chain transaction, call the
`sendasset` command followed by:
- the asset's contract ID
- the amount to be sent
- the recipient's blinded UTXO

Example:
```
sendasset rgb1lfxs4dmqs7a90vrz0yaje60fakuvu9u9esx882shy437yxazmysqamnv2r 400 txob1y3w8h9n4v4tkn37uj55dvqyuhvftrr2cxecp4pzkhjxjc4zcfxtsmdt2vf
```

### Refreshing a transfer
Transfers complete automatically on the sender side after the `sendasset`
command. To complete a transfer on the receiver side, once the send operation
is complete, call the `refresh` command.

Example:
```
refresh
```

### Showing an asset's balance
To show an asset's balance, call the `assetbalance` command followed by the
asset's contract ID for which the balance should be displayed.

Example:
```
assetbalance rgb1lfxs4dmqs7a90vrz0yaje60fakuvu9u9esx882shy437yxazmysqamnv2r
```

### Mining blocks
A command to mine new blocks is provided for convenience. To mine new blocks,
call the `mine` command followed by the desired number of blocks.

Example:
```
mine 6
```

## Off-chain operations

Off-chain RGB operations are available as modified CLI commands. The following
sections briefly explain which commands support RGB functionality and how to use
them.

### Opening channels
To open a new channel, call the `openchannel` command followed by:
- the peer's pubkey, host and port
- the bitcoin amount to allocate to the channel, in satoshis
- the bitcoin amount to push, in millisatoshis
- the RGB asset's contract ID
- the RGB amount to allocate to the channel
- the `--public` optional flag, to announce the channel

Example:
```
openchannel 03ddf2eedb06d5bbd128ccd4f558cb4a7428bfbe359259c718db7d2a8eead169fb@127.0.0.1:9736 999666 546000 rgb1lfxs4dmqs7a90vrz0yaje60fakuvu9u9esx882shy437yxazmysqamnv2r
zmysqamnv2r 100 --public
```

### Listing channels
To list the available channels, call the `listchannels` command. The output
contains RGB information about the channel:
- `rgb_contract_id`: the asset's contract ID
- `rgb_local_amount`: the amount allocated to the local peer
- `rgb_remote_amount`: the amount allocated to the remote peer

Example:
```
listchannels
```

### Sending assets
To send RGB assets over the LN network, call the `coloredkeysend` command followed by:
- the receiving peer's pubkey
- the bitcoin amount in satoshis
- the RGB asset's contract ID
- the RGB amount

Example:
```
coloredkeysend 03ddf2eedb06d5bbd128ccd4f558cb4a7428bfbe359259c718db7d2a8eead169fb 2000000 rgb1lfxs4dmqs7a90vrz0yaje60fakuvu9u9esx882shy437yxazmysqamnv2r 10
```

At the moment, only the `coloredkeysend` command has been modified to support RGB
functionality. The invoice-based `sendpayment` will be added in the future.

### Closing channels
To close a channel, call the `closechannel` (for a cooperative close) or the `forceclosechannel` (for a unilateral close) command followed by:
- the channel ID
- the peer's pubkey

Example (cooperative):
```
closechannel 83034b8a3302bb9cc63d75ffd49b03e224cb28d4911702827a8dd2553d0f5229 03ddf2eedb06d5bbd128ccd4f558cb4a7428bfbe359259c718db7d2a8eead169fb
```

Example (unilateral):
```
forceclosechannel 83034b8a3302bb9cc63d75ffd49b03e224cb28d4911702827a8dd2553d0f5229 03ddf2eedb06d5bbd128ccd4f558cb4a7428bfbe359259c718db7d2a8eead169fb
```

## Scripted tests

A few scenarios can be tested using a scripted sequence. This is only supported
on regtest as mining needs to be automated as well.

The entrypoint for scripted tests is the shell command `tests/test.sh`,
which can be called from the project's root directory. The default network is
"regtest" so it is not mandatory to specify it via the `--network` CLI option.

To view the available tests, call it with the `-l` option.
Example:
```sh
tests/test.sh -l
```

To start a test, call it with:
- the `-t` option followed by the test name
- the `--start` option to automatically create directories and start the services
- the `--stop` option to automatically stop the services and cleanup
Example:
```sh
tests/test.sh -t multihop --start --stop
```

To get a help message, call it with the `-h` option.
Example:
```sh
tests/test.sh -h
```

## License

Licensed under either:

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

[RGB proxy server]: https://github.com/grunch/rgb-proxy-server
[ldk-sample]: https://github.com/lightningdevkit/ldk-sample
[off-chain]: #off-chain-operations
[on-chain]: #on-chain-operations
[rgb-lightning-node]: https://github.com/RGB-Tools/rgb-lightning-node
[rust-lightning]: https://github.com/lightningdevkit/rust-lightning
