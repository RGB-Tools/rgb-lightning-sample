#!/usr/bin/env bash

source tests/common.sh


get_node_ids

# create RGB UTXOs
create_utxos 1
#create_utxos 2
create_utxos 3

# issue asset
issue_asset

# open channel
open_colored_channel 1 2 "$NODE2_PORT" "$NODE2_ID" 600
list_channels 1
list_channels 2
asset_balance 1 400

# send assets
colored_keysend 1 2 "$NODE2_ID" 100
list_channels 1
list_channels 2
list_payments 1
list_payments 2

# force-close channel
forceclose_channel 1 2 "$NODE2_ID"
asset_balance 1 900
asset_balance 2 100

exit 0
