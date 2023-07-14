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
open_colored_channel 2 3 "$NODE3_PORT" "$NODE3_ID" 400 1
channel23_id="$CHANNEL_ID"
list_channels 2 2
list_channels 3
asset_balance 2 0

# send assets and exit intermediate node while payment is goind through
colored_keysend_init 1 3 "$NODE3_ID" 50
_wait_for_text "$T_1" node2 "HANDLED UPDATE ADD HTLC"
timestamp
_wait_for_text "$T_1" node2 "HANDLED REVOKE AND ACK"
timestamp
_subtit "exit from node 2"
$TMUX_CMD send-keys -t node2 "" >/dev/null 2>&1
timestamp
sleep 10
list_channels 1
list_payments 1
_wait_for_text_multi 5 node1 "listpayments" "htlc_direction: outbound" >/dev/null
timestamp
_wait_for_text_multi 5 node1 "listpayments" "htlc_status: pending" >/dev/null
timestamp
asset_balance 1 100

# force-close channel from node 1
forceclose_channel_init 1 "$NODE2_ID" "$channel12_id"

mine 144
_wait_for_text_multi "$T_1" node1 "mine" "Event::SpendableOutputs complete"
timestamp
mine 1
asset_balance 1 550

mine 100
_wait_for_text_multi "$T_1" node1 "mine" "COMPLETED GET_FULLY_SIGNED_HTLC_TX"
timestamp

mine 144
_wait_for_text_multi "$T_1" node1 "mine" "Event::SpendableOutputs complete"
timestamp
mine 1
asset_balance 1 600

exit 0
