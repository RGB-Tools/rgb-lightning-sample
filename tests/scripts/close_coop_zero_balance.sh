#!/usr/bin/env bash

source tests/common.sh


get_node_ids

# create RGB UTXOs
create_utxos 1
create_utxos 2

# issue asset
issue_asset

# open channel
open_colored_channel 1 2 "$NODE2_PORT" "$NODE2_ID" 1000
list_channels 1
list_channels 2
asset_balance 1 0

# close channel
close_channel 1 2 "$NODE2_ID"
asset_balance 1 1000
asset_balance 2 0

exit 0
