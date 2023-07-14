#!/usr/bin/env bash

source tests/common.sh


get_node_ids

_tit "restart nodes"
exit_node 1
start_node 1
exit_node 2
start_node 2
exit_node 3
start_node 3

# create RGB UTXOs
create_utxos 1
create_utxos 2
create_utxos 3

# issue asset
issue_asset
_tit "restart nodes"
exit_node 1
start_node 1
exit_node 2
start_node 2
exit_node 3
start_node 3
asset_balance 1 1000

# open channel
open_colored_channel 1 2 "$NODE2_PORT" "$NODE2_ID" 600
_tit "restart nodes"
exit_node 1
start_node 1

check_channel_reestablish 1 "LDK startup successful."
exit_node 2
start_node 2
check_channel_reestablish 2 "LDK startup successful."
exit_node 3
start_node 3
list_channels 1
list_channels 2
asset_balance 1 400

# send assets
colored_keysend 1 2 "$NODE2_ID" 100
_tit "restart nodes"
exit_node 1
start_node 1
exit_node 2
start_node 2
exit_node 3
start_node 3
list_channels 1
list_channels 2
# TODO: pending https://github.com/lightningdevkit/ldk-sample/pull/104
#list_payments 1
#list_payments 2

# close channel
close_channel 1 2 "$NODE2_ID"
sleep 3
_tit "restart nodes"
exit_node 1
start_node 1
exit_node 2
start_node 2
exit_node 3
start_node 3
asset_balance 1 900
asset_balance 2 100

# spend RGB assets on-chain
_skip_remaining
blind 3
send_assets 1 700
blind 3
send_assets 2 50
mine 1
refresh 3
_tit "restart nodes"
exit_node 1
start_node 1
exit_node 2
start_node 2
exit_node 3
start_node 3
asset_balance 1 200
asset_balance 2 50
asset_balance 3 750

exit 0
