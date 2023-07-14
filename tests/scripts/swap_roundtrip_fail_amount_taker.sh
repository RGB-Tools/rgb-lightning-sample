#!/usr/bin/env bash

source tests/common.sh


get_node_ids

# create RGB UTXOs
create_utxos 1
create_utxos 2

# issue asset
issue_asset

# open channel
open_big_colored_channel 1 2 "$NODE2_PORT" "$NODE2_ID" 600
list_channels 1 1
list_channels 2 1

open_vanilla_channel 2 1 "$NODE1_PORT" "$NODE1_ID" 16777215
list_channels 2 2
list_channels 1 2

maker_init 2 1000 "sell" 100
# The amount is too large, we don't have enough
taker_amount_failure 1
