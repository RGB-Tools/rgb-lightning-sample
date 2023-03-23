#!/usr/bin/env bash
#
# automated tests for RGB LN node PoC
#
# note: in case something goes wrong, the test won't handle the resulting
#       situation and might leave one or more running processes; if that
#       happens use the "dislocate" command to re-attach to those processes and
#       exit them

prog=$(realpath "$(dirname "$0")")
name=$(basename "$0")
bad_net_msg="incorrect network; available networks: testnet, regtest"

BITCOIN_CLI="docker compose exec -u blits bitcoind bitcoin-cli -regtest"
NETWORK="regtest"
TMUX_CMD="tmux -L rgb-tmux"
export BITCOIN_CLI NETWORK TMUX_CMD


_die () {
    echo "ERR: $*"
    exit 1
}

_handle_network() {
    set -a
    case $NETWORK in
        regtest)
            RGB_ELECTRUM_SERVER=electrs:50001
            ELECTRUM_URL=electrs
            ELECTRUM_PORT=50001
            ;;
        testnet)
            RGB_ELECTRUM_SERVER=ssl://electrum.iriswallet.com:50013
            ELECTRUM_URL=ssl://electrum.iriswallet.com
            ELECTRUM_PORT=50013
            ;;
        *)
            _die "$bad_net_msg"
            ;;
    esac
    set +a
}

_start_services() {
    _stop_services

    mkdir -p data{rgb0,rgb1,rgb2,core,index,ldk0,ldk1,ldk2}
    case $NETWORK in
        regtest)
            docker compose --profile=regtest up -d
            echo
            echo "preparing bitcoind wallet"
            $BITCOIN_CLI createwallet miner >/dev/null
            $BITCOIN_CLI -rpcwallet=miner -generate 103 >/dev/null
            ;;
        testnet)
            docker compose up -d
            ;;
        *)
            _die "$bad_net_msg"
            ;;
    esac
}

_start_tmux() {
    _stop_tmux

    echo "starting tmux"
    $TMUX_CMD -f tests/tmux.conf new-session -d -n node1 -s rgb-lightning-sample -x 200 -y 100
    $TMUX_CMD send-keys 'target/debug/ldk-sample user:password@localhost:18443 dataldk0/ 63963 9735 regtest' C-m
    $TMUX_CMD new-window -n node2
    $TMUX_CMD send-keys 'target/debug/ldk-sample user:password@localhost:18443 dataldk1/ 63964 9736 regtest' C-m
    $TMUX_CMD new-window -n node3
    $TMUX_CMD send-keys 'target/debug/ldk-sample user:password@localhost:18443 dataldk2/ 63965 9737 regtest' C-m
    sleep 1

    echo
    echo "to attach the tmux session, execute \"$TMUX_CMD attach-session -t rgb-lightning-sample\""
}

_stop_services() {
    docker compose down --remove-orphans
    rm -rf data{rgb0,rgb1,rgb2,core,index,ldk0,ldk1,ldk2}
}

_stop_tmux() {
    echo
    echo "stopping tmux"
    $TMUX_CMD kill-server >/dev/null 2>&1
    sleep 1
}

_cleanup() {
    # cd back to calling directory
    cd - >/dev/null || exit 1
}

_help() {
    echo "$name [-h|--help]"
    echo "    show this help message"
    echo
    echo "$name [-d|--debug]"
    echo "    enable test debug output"
    echo
    echo "$name [-l|--list]"
    echo "    list available test scripts"
    echo
    echo "$name [-n|--network]"
    echo "    choose the bitcoin network to be used"
    echo "    available options: regtest (default), testnet"
    echo
    echo "$name [-t|--test <test_name>] [--start] [--stop]"
    echo "    build and exit if an error is returned"
    echo "    -t      run test with provided name"
    echo "    --start stop services, clean up, start services,"
    echo "            create bitcoind wallet used for mining,"
    echo "            generate initial blocks"
    echo "    --stop  stop services and clean up"
}

# cmdline arguments
while [ -n "$1" ]; do
    case $1 in
        -h|--help)
            _help
            exit 0
            ;;
        -d|--debug)
            export DEBUG=1
            ;;
        -l|--list-tests)
            scripts=$(find "${prog}/scripts/" -name '*.sh' -printf '%f\n' | sort)
            echo "list of available test scripts:"
            for s in $scripts; do
                echo " - ${s%.sh}"
            done
            exit 0
            ;;
        -n|--network)
            [ "$2" = "regtest" ] || [ "$2" = "testnet" ] || _die "$bad_net_msg"
            NETWORK="$2"
            shift
            ;;
        --start)
            start=1
            ;;
        --stop)
            stop=1
            ;;
        -t|--test)
            export NETWORK=regtest
            script_name="$2"
            script="${prog}/scripts/${script_name}.sh"
            [ -r "$script" ] || _die "script \"$script\" not found"
            shift
            ;;
        *)
            _die "unsupported argument \"$1\""
            ;;
    esac
    shift
done

# make sure to cleanup on exit
trap _cleanup EXIT

# cd to project root
cd "$prog/.." || exit

# check network and set env variables accordingly
_handle_network

# build project (if test run has been requested)
if [ -n "$script" ]; then
    cargo build || exit 1
fi

# start services if requested
[ "$start" = "1" ] && _start_services

# start test script if requested (regtest only)
if [ -n "$script" ] && [ -n "$script_name" ]; then
    [ "$NETWORK" = "regtest" ] || _die "tests are only available on regtest"
    echo
    echo "starting test script: $script_name"
    _start_tmux
    bash "$script"
fi

# stop services if requested
if [ "$stop" = "1" ]; then
    echo
    echo "services will now be stopped"
    echo
    read -p "press <enter> to continue"
    _stop_services
    _stop_tmux
fi

exit 0
