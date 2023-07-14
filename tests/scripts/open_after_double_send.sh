#!/usr/bin/env bash

source tests/common.sh


get_node_ids

# create RGB UTXOs
create_utxos 1
create_utxos 2
create_utxos 3

# issue asset
issue_asset

# send assets (1)
blind 2
send_assets 1 100
asset_balance 1 900

# send assets (2)
blind 2
send_assets 1 200
asset_balance 1 700

refresh 2
asset_balance 2 300

# open channel
open_colored_channel 2 1 "$NODE1_PORT" "$NODE1_ID" 250
list_channels 1
list_channels 2
asset_balance 1 700
asset_balance 2 50

## send assets
colored_keysend 2 1 "$NODE1_ID" 50
list_channels 1
list_channels 2
list_payments 1
list_payments 2

## close channel
close_channel 1 2 "$NODE2_ID"
asset_balance 1 750
asset_balance 2 250

## spend RGB assets on-chain
_skip_remaining
blind 3
send_assets 1 725
blind 3
send_assets 2 225
mine 1
refresh 3
asset_balance 1 25
asset_balance 2 25
asset_balance 3 950

exit 0
