#!/use/bin/env bash


# BITCOIN_CLI, COMPOSE and TMUX_CMD vars expected to be set in the environment
VERBOSE=${VERBOSE:-0}
TIMESTAMP=$(date +%s%3N)

T_1=30

NODE1_PORT=9735
NODE2_PORT=9736
NODE3_PORT=9737

# shell colors
C0='\033[0;31m' # red
C1='\033[0;32m' # green
C2='\033[0;33m' # orange
C3='\033[0;34m' # blue
NC='\033[0m'    # No Color


_exit() {
    timestamp
    _latest_logs
    echo
    printf "\n${C0}ERR: %s${NC}\n" "$@"
    exit 3
}

_tit() {
    printf "\n${C3}========[ %s ]${NC}\n" "$@"
}

_subtit() {
    printf "${C2}==== %s${NC}\n" "$@"
}

_debug() {
    [ "$VERBOSE" != 0 ] && printf "== %s\n" "$@" >&2
}

_out() {
    printf "${C1}--> %s${NC}\n" "$@"
}

_get_last_text() {
    local pane pattern
    pane="$1"
    pattern="$2"
    $TMUX_CMD capture-pane -ep -t "$pane" -S -20 \
        | tac | sed -n "0,/$pattern/p" | tac
}
export -f _get_last_text

_latest_logs() {
    [ "$VERBOSE" = 0 ] && return
    _tit "latest node logs"
    for node_num in 1 2 3; do
        _subtit "node $node_num"
        $TMUX_CMD capture-pane -ep -t "node$node_num" -S -20 | grep -v '^$'
    done
}

_wait_for_text() {
    local timeout pane pattern lines
    timeout="$1"
    pane="$2"
    pattern="$3"
    lines="${4:-0}"
    _debug "expecting \"$pattern\""
    timeout  --foreground "$timeout" bash <<EOT
    while :; do
        _get_last_text "$pane" "$pattern" | grep -A$lines "$pattern" && break
        sleep 1
    done
EOT
    [ $? = 0 ] || _exit "expected output ($pattern) not found, exiting..."
}

_wait_for_text_multi() {
    local timeout pane pattern pattern_pre lines
    timeout="$1"
    pane="$2"
    pattern_pre="$3"
    pattern="$4"
    lines="${5:-0}"
    _debug "expecting \"$pattern\""
    timeout  --foreground "$timeout" bash <<EOT
    while :; do
        _get_last_text "$pane" "$pattern_pre" | grep -A$lines "$pattern" && break
        sleep 1
    done
EOT
    [ $? = 0 ] || _exit "expected output ($pattern) not found, exiting..."
}

_skip_remaining() {
    [ "$SKIP" = 1 ] && _subtit "skipping final test portion" && exit 0
}


timestamp() {
    local prev_time
    prev_time=$TIMESTAMP
    TIMESTAMP=$(date +%s%3N)
    if [ "$VERBOSE" != 0 ]; then
        echo -n "$(date +%T)"
        echo " ($((TIMESTAMP-prev_time)) ms)"
    fi

}

check() {
    local num="$1"
    _subtit "checking output from node $num"
}

get_address() {
    local num="$1"
    _subtit "getting an address from node $num"
    $TMUX_CMD send-keys -t "node$num" "getaddress" C-m
    address=$(_wait_for_text_multi 5 "node$num" "getaddress" "Address:" \
        | head -1 | grep -Eo '[0-9a-z]{40,48}')
    _out "address: $address"
}

fund_address() {
    local address txid
    address="$1"
    _subtit "funding address $address"
    txid=$($BITCOIN_CLI sendtoaddress "$address" 1)
    _out "txid: $txid"
    mine 1
}

create_utxos() {
    local num get_funds
    num="$1"
    get_funds="${2:-1}"
    _tit "creating UTXOs on node $num"
    if [ "$get_funds" != 0 ]; then
        get_address "$num"
        fund_address "$address"
    fi
    _subtit "calling createutxos"
    $TMUX_CMD send-keys -t "node$num" "createutxos" C-m
    timestamp
    _wait_for_text_multi $T_1 "node$num" "createutxos" "UTXO creation complete"
    timestamp
    mine 1
}

get_node_ids() {
    local t_id=15
    _tit "get node IDs"
    NODE1_ID=$(_wait_for_text_multi $t_id node1 \
        "LDK startup successful." "Local Node ID is" |awk '{print $NF}')
    NODE2_ID=$(_wait_for_text_multi $t_id node2 \
        "LDK startup successful." "Local Node ID is" |awk '{print $NF}')
    NODE3_ID=$(_wait_for_text_multi $t_id node3 \
        "LDK startup successful." "Local Node ID is" |awk '{print $NF}')
    _out "node 1 ID: $NODE1_ID"
    _out "node 2 ID: $NODE2_ID"
    _out "node 3 ID: $NODE3_ID"
}

mine() {
    local blocks
    blocks="$1"
    _subtit "mining $blocks block(s)"
    $TMUX_CMD send-keys -t node1 "mine $blocks" C-m
    HEIGHT=$((HEIGHT+blocks))
    until $COMPOSE logs --tail=100 electrs |grep "chain updated: .* height=$HEIGHT" >/dev/null; do
        sleep 1
    done
}

issue_asset() {
    local rgb_amt=1000
    _tit "issue RGB asset ($rgb_amt)"
    $TMUX_CMD send-keys -t node1 "issueasset $rgb_amt USDT Tether 0" C-m
    ASSET_ID=$(_wait_for_text_multi 20 node1 "issueasset" "Asset ID:" |awk '{print $NF}')
    _out "asset ID: $ASSET_ID"
}

asset_balance() {
    local num expected balance
    num="$1"
    expected="${2:--1}"
    _subtit "asset balance on node $num"
    $TMUX_CMD send-keys -t "node$num" "assetbalance $ASSET_ID" C-m
    balance="$(_wait_for_text_multi $T_1 "node$num" "assetbalance" "Asset balance"\
        | grep -Eo '[0-9]+$')"
    _out "asset balance: $balance"
    if [ "$expected" != -1 ]; then
        [ "$balance" != "$expected" ] && \
            _exit "balance does not match the expected one ($expected)"
    fi
    _wait_for_text_multi $T_1 "node$num" "assetbalance" ">" >/dev/null
}

blind() {
    local num="$1"
    _tit "generate blinded UTXO on node $num"
    $TMUX_CMD send-keys -t "node$num" "receiveasset" C-m
    blinded_utxo="$(_wait_for_text_multi $T_1 "node$num" "receiveasset" "Blinded UTXO:" \
        | grep -Eo '[0-9a-zA-Z]+$')"
    _out "blinded UTXO: $blinded_utxo"
}

send_assets() {
    local num rgb_amt
    num="$1"
    rgb_amt="$2"
    _tit "send $rgb_amt assets on-chain from node $num to blinded UTXO $blinded_utxo"
    $TMUX_CMD send-keys -t "node$num" "sendasset $ASSET_ID $rgb_amt $blinded_utxo" C-m
    timestamp
    check "$num"
    _wait_for_text_multi $T_1 "node$num" "sendasset" "RGB send complete"
    timestamp
}

refresh() {
    local num="$1"
    _tit "refresh on node $num"
    $TMUX_CMD send-keys -t "node$num" "refresh" C-m
    timestamp
    check "$num"
    _wait_for_text_multi $T_1 "node$num" "refresh" "Refresh complete"
    timestamp
}

open_colored_channel() {
    open_colored_channel_custom_sats_amt $1 $2 $3 $4 $5 30010 $6
}

open_big_colored_channel() {
    open_colored_channel_custom_sats_amt $1 $2 $3 $4 $5 16777215 $6
}

open_colored_channel_custom_sats_amt() {
    local src_num dst_num dst_port dst_id rgb_amt sats_amt current_chan_num
    src_num="$1"
    dst_num="$2"
    dst_port="$3"
    dst_id="$4"
    rgb_amt="$5"
    sats_amt="$6"
    current_chan_num="${7:-0}"
    _tit "open channel from node $src_num to node $dst_num with $rgb_amt assets"
    $TMUX_CMD send-keys -t "node$src_num" "opencoloredchannel $dst_id@127.0.0.1:$dst_port $sats_amt 1394000 $ASSET_ID $rgb_amt --public" C-m
    check "$src_num"
    _wait_for_text_multi $T_1 "node$src_num" "opencoloredchannel" "HANDLED ACCEPT CHANNEL"
    timestamp
    _wait_for_text_multi $T_1 "node$src_num" "opencoloredchannel" "FUNDING COMPLETED"
    timestamp
    _wait_for_text_multi $T_1 "node$src_num" "opencoloredchannel" "HANDLED FUNDING SIGNED"
    timestamp
    check "$dst_num"
    _wait_for_text $T_1 "node$dst_num" "HANDLED OPEN CHANNEL"
    timestamp
    _wait_for_text_multi $T_1 "node$dst_num" "HANDLED OPEN CHANNEL" "HANDLED FUNDING CREATED"
    timestamp

    mine 6
    check "$src_num"
    _wait_for_text_multi $T_1 "node$src_num" "> mine" "EVENT: Channel .* with peer .* is ready to be used"
    timestamp
    check "$dst_num"
    _wait_for_text_multi $T_1 "node$dst_num" "HANDLED OPEN CHANNEL" "EVENT: Channel .* with peer .* is ready to be used"
    timestamp

    local lines channels chan_peer_line chan_id_line
    $TMUX_CMD send-keys -t "node$src_num" "listchannels" C-m
    sleep 1
    lines=$(((current_chan_num+1)*20))
    channels="$(_wait_for_text 5 "node$src_num" "listchannels" $lines | sed -n '/^\[/,/^\]/p' | sed -n '/^\[/,/^\]/p')"
    chan_peer_line=$(echo "$channels" | grep -n "$dst_id" |cut -d: -f1)
    chan_id_line=$((chan_peer_line-2))
    CHANNEL_ID=$(echo "$channels" | sed -n "${chan_id_line},${chan_id_line}p" | grep -Eo '[0-9a-f]{64}')
    _out "channel ID: $CHANNEL_ID"
}

open_vanilla_channel() {
    local src_num dst_num dst_port dst_id msat_amount
    src_num="$1"
    dst_num="$2"
    dst_port="$3"
    dst_id="$4"
    msat_amount="$5"
    _tit "open channel from node $src_num to node $dst_num of $msat_amount mSAT"
    $TMUX_CMD send-keys -t "node$src_num" "openchannel $dst_id@127.0.0.1:$dst_port $msat_amount 546000 --public" C-m
    check $src_num
    _wait_for_text_multi $T_1 "node$src_num" "openchannel" "HANDLED ACCEPT CHANNEL"
    timestamp
    _wait_for_text_multi $T_1 "node$src_num" "openchannel" "FUNDING COMPLETED"
    timestamp
    _wait_for_text_multi $T_1 "node$src_num" "openchannel" "HANDLED FUNDING SIGNED"
    timestamp
    check $dst_num
    _wait_for_text $T_1 "node$dst_num" "HANDLED OPEN CHANNEL"
    timestamp
    _wait_for_text_multi $T_1 "node$dst_num" "HANDLED OPEN CHANNEL" "HANDLED FUNDING CREATED"
    timestamp

    mine 6
    sleep 3

    $TMUX_CMD send-keys -t "node$src_num" "listchannels" C-m
    sleep 1
    CHANNEL_ID=$(_wait_for_text 5 "node$src_num" "[^_]channel_id:" \
        | head -1 | grep -Eo '[0-9a-f]{64}')
    _out "channel ID: $CHANNEL_ID"
}


list_channels() {
    local node_num chan_num lines text matches
    node_num="$1"
    chan_num="${2:-1}"
    lines=$((chan_num*20))
    _subtit "list channels ($chan_num expected) on node $node_num"
    $TMUX_CMD send-keys -t "node$node_num" "listchannels" C-m
    sleep 1
    text="$(_wait_for_text 5 "node$node_num" "listchannels" $lines | sed -n '/^\[/,/^\]/p')"
    echo "$text"
    matches=$(echo "$text" | grep -c "is_channel_ready: true")
    [ "$matches" = "$chan_num" ] || _exit "one or more channels not ready"
}

list_payments() {
    local node_num payment_num lines text matches
    node_num="$1"
    payment_num="${2:-1}"
    lines=$((payment_num*10))
    _tit "list payments on node $node_num"
    $TMUX_CMD send-keys -t "node$node_num" "listpayments" C-m
    text="$(_wait_for_text 5 "node$node_num" "listpayments" $lines | sed -n '/^\[/,/^\]/p')"
    echo "$text"
    matches=$(echo "$text" | grep -c "payment_hash:")
    [ "$matches" = "$payment_num" ] || _exit "payment number doesn't match the expected one"
    _wait_for_text_multi $T_1 "node$node_num" "listpayments" ">" >/dev/null
}

list_unspents() {
    [ "$VERBOSE" = 0 ] && return
    local num="$1"
    _tit "list unspents on node $num"
    $TMUX_CMD send-keys -t "node$num" "listunspent" C-m
    _wait_for_text_multi 5 "node$num" "listunspent" "Unspents:" 40 | sed -n '/^Unspents:/,/^>/p'
}

close_channel() {
    local src_num dst_num dst_id chan_id
    src_num="$1"
    dst_num="$2"
    dst_id="$3"
    chan_id="${4:-$CHANNEL_ID}"
    _tit "close channel with peer $dst_id from node $src_num (cooperative)"
    $TMUX_CMD send-keys -t "node$src_num" "closechannel $chan_id $dst_id" C-m
    timestamp
    check "$src_num"
    _wait_for_text_multi $T_1 "node$src_num" "closechannel" "HANDLED SHUTDOWN"
    timestamp
    check "$dst_num"
    _wait_for_text_multi $T_1 "node$dst_num" "HANDLED SHUTDOWN" "EVENT: Channel .* closed due to: CooperativeClosure"
    timestamp

    mine 6
    check "$dst_num"
    _wait_for_text_multi $T_1 "node$dst_num" "EVENT: Channel .* closed" "Event::SpendableOutputs complete"
    timestamp

    check "$src_num"
    _wait_for_text_multi $T_1 "node$src_num" "EVENT: Channel .* closed" "Event::SpendableOutputs complete"
    timestamp
    mine 1
}

forceclose_channel_init() {
    local src_num dst_id chan_id
    src_num="$1"
    dst_id="$2"
    chan_id="$3"
    _tit "close channel from node $src_num (unilateral)"
    $TMUX_CMD send-keys -t "node$src_num" "forceclosechannel $chan_id $dst_id" C-m
    timestamp
    check "$src_num"
    _wait_for_text_multi $T_1 "node$src_num" "forceclosechannel" "EVENT: Channel .* closed due to: HolderForceClosed"
    timestamp
}

forceclose_channel() {
    local src_num dst_num dst_id chan_id
    src_num="$1"
    dst_num="$2"
    dst_id="$3"
    chan_id="${4:-$CHANNEL_ID}"

    forceclose_channel_init "$src_num" "$dst_id" "$chan_id"

    check "$dst_num"
    _wait_for_text $T_1 "node$dst_num" "EVENT: Channel .* closed due to: CounterpartyForceClosed"
    timestamp

    mine 6
    check "$dst_num"
    _wait_for_text_multi $T_1 "node$dst_num" "EVENT: Channel .* closed" "Event::SpendableOutputs complete"
    timestamp

    mine 144
    check "$src_num"
    _wait_for_text_multi $T_1 "node$src_num" "forceclosechannel" "Event::SpendableOutputs complete"
    timestamp
    mine 1
}

colored_keysend_init() {
    local src_num dst_num dst_id rgb_amt
    src_num="$1"
    dst_num="$2"
    dst_id="$3"
    rgb_amt="$4"

    _tit "send $rgb_amt assets off-chain from node $src_num to node $dst_num"
    $TMUX_CMD send-keys -t "node$src_num" "coloredkeysend $dst_id 3000000 $ASSET_ID $rgb_amt" C-m
    timestamp
    check "$src_num"
    _wait_for_text_multi $T_1 "node$src_num" "keysend" "EVENT: initiated sending"
    timestamp
}

colored_keysend() {
    local src_num dst_num dst_id rgb_amt
    src_num="$1"
    dst_num="$2"
    dst_id="$3"
    rgb_amt="$4"

    colored_keysend_init "$src_num" "$dst_num" "$dst_id" "$rgb_amt"

    _wait_for_text_multi $T_1 "node$src_num" "keysend" "EVENT: successfully sent payment"
    timestamp
    _wait_for_text_multi $T_1 "node$src_num" "EVENT: successfully sent payment" "HANDLED REVOKE AND ACK"
    timestamp

    check "$dst_num"
    _wait_for_text $T_1 "node$dst_num" "EVENT: received payment"
    timestamp
    _wait_for_text_multi $T_1 "node$dst_num" "EVENT: received payment" "Event::PaymentClaimed end"
    timestamp
    _wait_for_text_multi $T_1 "node$dst_num" "Event::PaymentClaimed end" "HANDLED COMMITMENT SIGNED"
    timestamp
}

keysend_init() {
    local src_num dst_num dst_id sats_amt
    src_num="$1"
    dst_num="$2"
    dst_id="$3"
    sats_amt="$4"

    _tit "send $sats_amt sats off-chain from node $src_num to node $dst_num"
    $TMUX_CMD send-keys -t "node$src_num" "keysend $dst_id $sats_amt" C-m
    timestamp
    check "$src_num"
    _wait_for_text_multi $T_1 "node$src_num" "keysend" "EVENT: initiated sending"
    timestamp
}

keysend() {
    local src_num dst_num dst_id sats_amt
    src_num="$1"
    dst_num="$2"
    dst_id="$3"
    sats_amt="$4"

    keysend_init "$src_num" "$dst_num" "$dst_id" "$sats_amt"

    _wait_for_text_multi $T_1 "node$src_num" "keysend" "EVENT: successfully sent payment"
    timestamp
    _wait_for_text_multi $T_1 "node$src_num" "EVENT: successfully sent payment" "HANDLED REVOKE AND ACK"
    timestamp

    check "$dst_num"
    _wait_for_text $T_1 "node$dst_num" "EVENT: received payment"
    timestamp
    _wait_for_text_multi $T_1 "node$dst_num" "EVENT: received payment" "Event::PaymentClaimed end"
    timestamp
    _wait_for_text_multi $T_1 "node$dst_num" "Event::PaymentClaimed end" "HANDLED COMMITMENT SIGNED"
    timestamp
}

get_colored_invoice() {
    local num rgb_amt text pattern
    num="$1"
    rgb_amt="$2"

    _tit "get invoice for $rgb_amt assets from node $num"
    $TMUX_CMD send-keys -t "node$num" "getcoloredinvoice 3000000 900 $ASSET_ID $rgb_amt" C-m
    timestamp
    check "$num"
    pattern="SUCCESS: generated invoice: "
    INVOICE="$(_wait_for_text_multi $T_1 "node$num" \
        'getcoloredinvoice' "$pattern" 3 | sed "s/$pattern//" \
        |grep -Eo '^[0-9a-z]+$' | sed -E ':a; N; $!ba; s/[\n ]//g')"
    timestamp
    _out "invoice: $INVOICE"
}

get_vanilla_invoice() {
    local num msat_amount text pattern
    num="$1"
    msat_amount="$2"

    _tit "get invoice for $msat_amount mSATs from node $num"
    $TMUX_CMD send-keys -t node$num "getinvoice $msat_amount 900" C-m
    timestamp
    check "$num"
    pattern="SUCCESS: generated invoice: "
    INVOICE="$(_wait_for_text_multi $T_1 "node$num" \
        'getinvoice' "$pattern" 3 | sed "s/$pattern//" \
        |grep -Eo '^[0-9a-z]+$' | sed -E ':a; N; $!ba; s/[\n ]//g')"
    timestamp
    _out "invoice: $INVOICE"
}

send_payment() {
    local src_num dst_num invoice
    src_num="$1"
    dst_num="$2"
    invoice="$3"

    _tit "pay LN invoice from node $src_num"
    $TMUX_CMD send-keys -t "node$src_num" "sendpayment $invoice" C-m
    timestamp
    check "$src_num"
    _wait_for_text_multi $T_1 "node$src_num" "sendpayment" "EVENT: initiated sending"
    timestamp
    _wait_for_text_multi $T_1 "node$src_num" "sendpayment" "EVENT: successfully sent payment"
    timestamp

    check "$dst_num"
    _wait_for_text $T_1 "node$dst_num" "EVENT: received payment"
    timestamp
    _wait_for_text_multi $T_1 "node$dst_num" "EVENT: received payment" "Event::PaymentClaimed end"
    timestamp
    _wait_for_text_multi $T_1 "node$dst_num" "Event::PaymentClaimed end" "HANDLED COMMITMENT SIGNED"
    timestamp
}

exit_node() {
    local num
    num="$1"

    _subtit "exit node $num"
    $TMUX_CMD send-keys -t "node$num" "exit" C-m
    timestamp
    _wait_for_text_multi $T_1 "node$num" "exit" "Exiting node..." >/dev/null
    timestamp
}

start_node() {
    local num data port
    num="$1"

    case "$num" in
        1)
            data="dataldk0"
            port="9735"
            ;;
        2)
            data="dataldk1"
            port="9736"
            ;;
        3)
            data="dataldk2"
            port="9737"
            ;;
        *)
    esac

    _subtit "start node $num"
    $TMUX_CMD send-keys -t "node$num" \
        "target/debug/ldk-sample user:password@localhost:18443 $data/ $port regtest" C-m
    timestamp
    _wait_for_text_multi $T_1 "node$num" "target\/debug\/ldk-sample" "LDK startup successful." >/dev/null
    _wait_for_text_multi $T_1 "node$num" "LDK startup successful." ">" >/dev/null
    timestamp
}

check_channel_reestablish() {
    local num prevtext
    num="$1"
    prevtext="$2"

    check "$num"
    _wait_for_text_multi $T_1 "node$num" "$prevtext" "HANDLED CHANNEL READY" >/dev/null
    timestamp
}

send_swap() {
    local node exchange swaptype amt_msat amt_asset
    node="$1"
    exchange="$2"
    swaptype="$3"
    amt_msat="$4"
    amt_asset="$5"

    _tit "node $node swapping ($swaptype) $amt_msat msats for $amt_asset $ASSET_ID through node $exchange"
    $TMUX_CMD send-keys -t node$node "sendswap $exchange $swaptype $amt_msat $ASSET_ID $amt_asset" C-m
    timestamp
    check $node
    _wait_for_text_multi $T_1 node$node "sendswap" "EVENT: initiated swap"
    timestamp
    _wait_for_text_multi $T_1 node$node "sendswap" "EVENT: successfully sent payment"
    timestamp
    _wait_for_text $T_1 node$node "EVENT: received payment"
    timestamp
    _wait_for_text $T_1 node$node "Event::PaymentClaimed end"
    timestamp
    _wait_for_text_multi $T_1 node$node "Event::PaymentClaimed end" "HANDLED COMMITMENT SIGNED"
    timestamp
}

maker_init() {
    local node amount asset side price timeout
    node="$1"
    amount="$2"
    side="$3"
    timeout="$4"
    price="$5"

    _tit "node $node initializating trade (mm-side): swapping ($side) $amount of $ASSET_ID at $price msats/asset"
    timestamp
    $TMUX_CMD send-keys -t node$node "makerinit $amount $ASSET_ID $side $timeout $price" C-m
    swap_string=$(_wait_for_text 5 node$node "SUCCESS! swap_string =" |awk '{print $NF}')
    payment_secret=$(_wait_for_text 5 node$node "payment_secret: " |awk '{print $NF}')
    _out "swap_string: $swap_string"
    _out "payment_secret: $payment_secret"
    sleep 1
}
maker_init_amount_failure() {
    local node amount asset side price timeout
    node="$1"
    amount="$2"
    side="$3"
    timeout="$4"
    price="$5"

    _tit "node $node initializating trade (mm-side): swapping ($side) $amount of $ASSET_ID at $price msats/asset"
    timestamp
    $TMUX_CMD send-keys -t node$node "makerinit $amount $ASSET_ID $side $timeout $price" C-m
    _wait_for_text 5 node$node "ERROR: do not have enough RGB assets"
}

taker() {
    local node
    node="$1"

    _tit "node $node taking the trade $swap_string"
    $TMUX_CMD send-keys -t node$node "taker $swap_string" C-m
    taker_pk=$(_wait_for_text 5 node$node "our_pk: " |awk '{print $NF}')
    _out "taker_pk: $taker_pk"
    sleep 1
}

taker_expect_timeout() {
    local node
    node="$1"

    _tit "node $node taking the trade $swap_string"
    $TMUX_CMD send-keys -t node$node "taker $swap_string" C-m
    _wait_for_text_multi $T_1 node$node "taker" "ERROR: the swap offer has already expired"
    timestamp
}

taker_amount_failure() {
    local node
    node="$1"

    _tit "node $node taking the trade $swap_string"
    $TMUX_CMD send-keys -t node$node "taker $swap_string" C-m
    _wait_for_text 5 node$node "ERROR: do not have enough RGB assets"
}

taker_list() {
    local node text trades_num text
    node="$1"
    trades_num="$2"

    lines=$((trades_num*9))

    _tit "listing whitelisted taker trades on node $node"
    $TMUX_CMD send-keys -t node$node "tradeslist taker" C-m
    text="$(_wait_for_text 5 "node$node" "tradeslist taker" $lines)"
    echo "$text"
    matches=$(echo "$text" | grep -c "side: .*")
    [ "$matches" = "$trades_num" ] || _exit "not enough trades"
}

maker_list() {
    local node text trades_num text
    node="$1"
    trades_num="$2"

    lines=$((trades_num*10))

    _tit "listing whitelisted maker trades on node $node"
    $TMUX_CMD send-keys -t node$node "tradeslist maker" C-m
    text="$(_wait_for_text 5 "node$node" "tradeslist maker" $lines)"
    echo "$text"
    matches=$(echo "$text" | grep -c "side: .*")
    [ "$matches" = "$trades_num" ] || _exit "not enough trades"
}

maker_execute() {
    local node
    node="$1"

    _tit "node $node completing the trade..."
    $TMUX_CMD send-keys -t node$node "makerexecute $swap_string $payment_secret $taker_pk" C-m
    timestamp
    _wait_for_text_multi $T_1 node$node "makerexecute" "EVENT: initiated swap"
    timestamp
    _wait_for_text_multi $T_1 node$node "makerexecute" "EVENT: successfully sent payment"
    timestamp
    _wait_for_text $T_1 node$node "EVENT: received payment"
    timestamp
    _wait_for_text $T_1 node$node "Event::PaymentClaimed end"
    timestamp
    _wait_for_text_multi $T_1 node$node "Event::PaymentClaimed end" "HANDLED COMMITMENT SIGNED"
    timestamp

}

maker_execute_expect_failure() {
    local node taker_pk
    node="$1"
    taker_pk="$2"
    failure_node="$3"

    _tit "node $node completing the trade..."
    $TMUX_CMD send-keys -t node$node "makerexecute $swap_string $payment_secret $taker_pk" C-m
    timestamp
    _wait_for_text_multi $T_1 node$node "makerexecute" "EVENT: initiated swap"
    timestamp
    _wait_for_text $T_1 node$failure_node "ERROR: rejecting non-whitelisted swap"
    timestamp
    _wait_for_text_multi $T_1 node$node "makerexecute" "EVENT: Failed to send payment to payment hash .* RetriesExhausted>"
    timestamp

}
