#!/usr/bin/env bash

source tests/common.sh


get_node_ids

# create RGB UTXOs
create_utxos 1
create_utxos 2
create_utxos 3

# issue asset
issue_asset

# open channel
open_channel 1 2 "$NODE2_PORT" "$node2_id" 600
list_channels 1
list_channels 2

# get invoice
get_invoice 2 100

# send payment
send_payment 1 2 "$invoice"
list_channels 1
list_channels 2
list_payments 1
list_payments 2

# close channel
close_channel 1 2 "$node2_id"
asset_balance 1
asset_balance 2

# spend assets
blind 3
send_assets 1 700
blind 3
send_assets 2 50
mine 1
refresh 3
asset_balance 1
asset_balance 2
asset_balance 3
