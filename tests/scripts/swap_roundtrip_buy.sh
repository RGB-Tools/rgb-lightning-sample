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
open_big_colored_channel 1 2 "$NODE2_PORT" "$NODE2_ID" 600
list_channels 1 1
list_channels 2 1

open_vanilla_channel 2 3 "$NODE3_PORT" "$NODE3_ID" 16777215
list_channels 2 2
list_channels 3 1

open_vanilla_channel 3 1 "$NODE1_PORT" "$NODE1_ID" 16777215
list_channels 3 2
list_channels 1 2


maker_init 1 10 "buy" 900
taker 2
taker_list 2 1
maker_list 1 1
maker_execute 1

sleep 5

list_channels 2 2
list_channels 1 2
list_channels 3 2
