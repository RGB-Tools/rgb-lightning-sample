#!/usr/bin/env bash

source tests/common.sh


get_node_ids

get_address 1
fund_address $address
mine 1

# wait for bdk and ldk to sync up with electrs
sleep 5

# open channel
open_vanilla_channel 1 2 "$NODE2_PORT" "$NODE2_ID" 16777215
list_channels 1
list_channels 2

# get invoice
get_vanilla_invoice 2 3000000

# send payment
send_payment 1 2 "$INVOICE"
list_channels 1
list_channels 2
list_payments 1
list_payments 2

close_channel 1 2 "$NODE2_ID"