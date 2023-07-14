#!/usr/bin/env bash

source tests/common.sh


get_node_ids

# create RGB UTXOs
create_utxos 1
create_utxos 2
create_utxos 3

# issue asset
issue_asset

# send assets
blind 2
send_assets 1 400
asset_balance 1 600

# open channel
open_colored_channel 1 2 "$NODE2_PORT" "$NODE2_ID" 500
channel12_id="$CHANNEL_ID"
list_channels 1
list_channels 2
asset_balance 1 100

# refresh
refresh 2
asset_balance 2 400

# open channel
open_colored_channel 2 3 "$NODE3_PORT" "$NODE3_ID" 300 1
channel23_id="$CHANNEL_ID"
list_channels 2 2
list_channels 3
asset_balance 2 100


# send payment
get_colored_invoice 3 50
send_payment 1 3 "$INVOICE"

list_channels 1
list_channels 2 2
list_channels 3
list_payments 1
list_payments 3

# close channels
close_channel 2 1 "$NODE1_ID" "$channel12_id"
asset_balance 1 550
asset_balance 2 150
close_channel 3 2 "$NODE2_ID" "$channel23_id"
asset_balance 2 400
asset_balance 3 50

# spend RGB assets on-chain
_skip_remaining
blind 3
send_assets 1 200
blind 3
send_assets 2 150
mine 1
refresh 3
blind 2
send_assets 3 375
refresh 2
asset_balance 1 350
asset_balance 2 625
asset_balance 3 25

exit 0
