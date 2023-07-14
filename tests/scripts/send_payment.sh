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
open_colored_channel 1 2 "$NODE2_PORT" "$NODE2_ID" 600
list_channels 1
list_channels 2
asset_balance 1 400

# get invoice
get_colored_invoice 2 100

# send payment
send_payment 1 2 "$INVOICE"
list_channels 1
list_channels 2
list_payments 1
list_payments 2

# close channel
close_channel 1 2 "$NODE2_ID"
asset_balance 1 900
asset_balance 2 100

# spend assets
_skip_remaining
blind 3
send_assets 1 700
blind 3
send_assets 2 50
mine 1
refresh 3
asset_balance 1 200
asset_balance 2 50
asset_balance 3 750

exit 0
