#!/usr/bin/env bash

source tests/common.sh


get_node_ids

issue_asset

# send assets
blind 2
send_assets 1 400
asset_balance 1

# open channel
open_channel 1 2 "$NODE2_PORT" "$node2_id" 500
channel12_id="$channel_id"
list_channels 1
list_channels 2

# refresh
refresh 2
asset_balance 2

# open channel
open_channel 2 3 "$NODE3_PORT" "$node3_id" 400
channel23_id="$channel_id"
list_channels 2 2
list_channels 3

# send assets and exit intermediate node while payment is goind through
keysend_init 1 3 "$node3_id" 50
_wait_for_text $T_5 node2 "HANDLED UPDATE ADD HTLC"
timestamp
_wait_for_text $T_5 node2 "HANDLED REVOKE AND ACK"
timestamp
_subtit "exit from node 2"
$TMUX_CMD send-keys -t node2 "" >/dev/null 2>&1
timestamp
sleep $T_1
list_channels 1
list_payments 1
_wait_for_text $T_1 node1 "htlc_direction: outbound" >/dev/null
timestamp
_wait_for_text $T_1 node1 "htlc_status: pending" >/dev/null
timestamp
asset_balance 1

# force-close channel from node 1
channel_id="$channel12_id"
forceclose_channel_init 1 "$node2_id"

mine 144
_wait_for_text $T_5 node1 "Event::SpendableOutputs complete"
timestamp
asset_balance 1

mine 100
_wait_for_text $T_5 node1 "COMPLETED GET_FULLY_SIGNED_HTLC_TX"
timestamp
asset_balance 1

mine 144
_wait_for_text_multi $T_5 node1 "assetbalance" "Event::SpendableOutputs complete"
timestamp
asset_balance 1
