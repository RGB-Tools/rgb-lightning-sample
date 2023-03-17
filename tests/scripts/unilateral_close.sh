#!/usr/bin/env bash

source tests/common.sh


get_node_ids

# issue asset
issue_asset

# open channel
open_channel 1 2 "$NODE2_PORT" "$node2_id" 1000
list_channels 1
list_channels 2

# send assets
keysend 1 2 "$node2_id" 100
list_channels 1
list_channels 2
list_payments 1
list_payments 2

# force-close channel
forceclose_channel 1 2 "$node2_id"
asset_balance 1
asset_balance 2
