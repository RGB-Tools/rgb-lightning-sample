#!/usr/bin/env bash

source tests/common.sh


get_node_ids

issue_asset

# send assets 1->2 (1)
blind 2
send_assets 1 400
mine 1
asset_balance 1
refresh 2
asset_balance 2

# send assets 2->1 (1)
blind 1
send_assets 2 300
mine 1
asset_balance 2
refresh 1
asset_balance 1

# send assets 1->2 (2)
blind 2
send_assets 1 200
mine 1
asset_balance 1
refresh 2
asset_balance 2

# send assets 2->1 (2)
blind 1
send_assets 2 100
mine 1
asset_balance 2
refresh 1
asset_balance 1
