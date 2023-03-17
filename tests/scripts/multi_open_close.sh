#!/usr/bin/env bash

source tests/common.sh


get_node_ids

# issue asset
issue_asset

# open channel (1)
open_channel 1 2 "$NODE2_PORT" "$node2_id" 1000
list_channels 1
list_channels 2

# send assets (1)
keysend 1 2 "$node2_id" 100
list_channels 1
list_channels 2

# close channel (1)
close_channel 1 2 "$node2_id"
asset_balance 1
asset_balance 2

# open channel (2)
open_channel 1 2 "$NODE2_PORT" "$node2_id" 600
list_channels 1
list_channels 2

# send assets (2)
keysend 1 2 "$node2_id" 50
list_channels 1
list_channels 2

# close channel (2)
close_channel 1 2 "$node2_id"
asset_balance 1
asset_balance 2
