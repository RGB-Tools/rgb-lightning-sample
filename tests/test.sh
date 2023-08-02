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
build_log_file="cargo_build.log"

COMPOSE="docker compose"
if ! $COMPOSE >/dev/null; then
    echo "could not call docker compose (hint: install docker compose plugin)"
    exit 1
fi
BITCOIN_CLI="$COMPOSE exec -u blits bitcoind bitcoin-cli -regtest"
NETWORK="regtest"
INITIAL_BLOCKS=103
SUCCESSFUL_SCRIPTS=()
FAILED_SCRIPTS=()
TEST_PRINTS=1
TMUX_CMD="tmux -L rgb-tmux"
export BITCOIN_CLI COMPOSE NETWORK TEST_PRINTS TMUX_CMD


_cleanup() {
    # cd back to calling directory
    cd - >/dev/null || exit 1
    # test run time in case the test ended abruptly
    _show_time " (interrupted)"
}

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

_find_scripts() {
    SCRIPTS=$(find "${prog}/scripts/" -name '*.sh' -printf '%f\n' | sort)
    SCRIPT_NUM=$(echo "$SCRIPTS" | wc -l)
}

_run_script() {
    local name file
    name="$1"
    file="$2"
    echo && echo "starting test script: $name"
    _stop_start_tmux
    TIME_START=$(date +%s)
    bash "$file"
    SCRIPT_EXIT="$?"
    _show_time " ($name)"
}

_show_time() {
    local msg="$1"
    if [ -n "$TIME_START" ]; then
        TIME_END="$(date +%s)"
        echo && echo "test run time$msg: $((TIME_END-TIME_START)) seconds"
        unset TIME_START TIME_END
    fi
}

_start_services() {
    _stop_services

    mkdir -p data{rgb0,rgb1,rgb2,core,index,ldk0,ldk1,ldk2}
    # see docker-compose.yml for the exposed ports
    EXPOSED_PORTS=(3000 50001)
    for port in "${EXPOSED_PORTS[@]}"; do
        if [ -n "$(ss -HOlnt "sport = :$port")" ];then
            _die "port $port is already bound, services can't be started"
        fi
    done
    case $NETWORK in
        regtest)
            $COMPOSE up -d
            echo && echo "preparing bitcoind wallet"
            $BITCOIN_CLI createwallet miner >/dev/null
            $BITCOIN_CLI -rpcwallet=miner -generate $INITIAL_BLOCKS >/dev/null
            export HEIGHT=$INITIAL_BLOCKS
            # wait for electrs to have completed startup
            until $COMPOSE logs electrs |grep 'finished full compaction' >/dev/null; do
                sleep 1
            done
            ;;
        *)
            _die "$bad_net_msg"
            ;;
    esac
}

_stop_services() {
    $COMPOSE down --remove-orphans
    rm -rf data{rgb0,rgb1,rgb2,core,index,ldk0,ldk1,ldk2}
}

_stop_start_tmux() {
    local bin_base="target/debug"
    [ "$RELEASE" = 1 ] && bin_base="target/release"
    _stop_tmux

    echo "starting tmux"
    $TMUX_CMD -f tests/tmux.conf new-session -d -n node1 -s rgb-lightning-sample -x 200 -y 100
    $TMUX_CMD send-keys "$bin_base/ldk-sample user:password@localhost:18443 dataldk0/ 9735 regtest" C-m
    $TMUX_CMD new-window -n node2
    $TMUX_CMD send-keys "$bin_base/ldk-sample user:password@localhost:18443 dataldk1/ 9736 regtest" C-m
    $TMUX_CMD new-window -n node3
    $TMUX_CMD send-keys "$bin_base/ldk-sample user:password@localhost:18443 dataldk2/ 9737 regtest" C-m
    sleep 1

    echo && echo "to attach the tmux session, execute \"$TMUX_CMD attach-session -t rgb-lightning-sample\""
}

_stop_tmux() {
    echo && echo "stopping tmux"
    $TMUX_CMD kill-server >/dev/null 2>&1
    sleep 1
}


_help() {
    echo "$name [-h|--help]"
    echo "    show this help message"
    echo
    echo "$name [-l|--list]"
    echo "    list available test scripts"
    echo
    echo "$name [-n|--network]"
    echo "    choose the bitcoin network to be used"
    echo "    available options: regtest (default), testnet"
    echo
    echo "$name [-r|--release]"
    echo "    build in release mode"
    echo
    echo "$name [-s|--skip-final-part]"
    echo "    skip final test portion (if supported by test)"
    echo
    echo "$name [-t|--test <test_name>] [--start] [--stop]"
    echo "    build and exit if an error is returned"
    echo "    -t      run test with provided name"
    echo "    -t all  run all tests (restarting services in between)"
    echo "    --start stop services, clean up, start services,"
    echo "            create bitcoind wallet used for mining,"
    echo "            generate initial blocks"
    echo "    --stop  stop services and clean up"
    echo
    echo "$name [-v|--verbose]"
    echo "    enable verbose output"
}

# cmdline arguments
[ -z "$1" ] && _help
while [ -n "$1" ]; do
    case $1 in
        -h|--help)
            _help
            exit 0
            ;;
        -l|--list-tests)
            _find_scripts
            echo "list of available test scripts:"
            for s in $SCRIPTS; do
                echo " - ${s%.sh}"
            done
            exit 0
            ;;
        -n|--network)
            [ "$2" = "regtest" ] || [ "$2" = "testnet" ] || _die "$bad_net_msg"
            NETWORK="$2"
            shift
            ;;
        -r|--release)
            RELEASE=1
            ;;
        -s|--skip-final-spend)
            export SKIP=1
            ;;
        -v|--verbose)
            export VERBOSE=1
            ;;
        --start)
            start=1
            ;;
        --stop)
            stop=1
            ;;
        -t|--test)
            script_name="$2"
            if [ "$script_name" = "all" ]; then
                script=$script_name
            else
                script="${prog}/scripts/${script_name}.sh"
                [ -r "$script" ] || _die "script \"$script_name\" not found"
            fi
            shift
            ;;
        *)
            _die "unsupported argument \"$1\""
            ;;
    esac
    shift
done

# make sure to cleanup on exit
trap _cleanup EXIT INT TERM

# cd to project root
cd "$prog/.." || exit

# check network and set env variables accordingly
_handle_network

# build project (if test run has been requested)
if [ -n "$script" ]; then
    echo -n "building project (see file $build_log_file for the build log)... "
    [ "$RELEASE" = 1 ] && release="--release"
    cargo build $release >$build_log_file 2>&1 || exit 1
    echo "done"
    echo
fi

# start services if requested
[ "$start" = "1" ] && _start_services

# start test script if requested (regtest only)
if [ -n "$script" ] && [ -n "$script_name" ]; then
    [ "$NETWORK" = "regtest" ] || _die "tests are only available on regtest"
    if [ "$script" = "all" ]; then
        _find_scripts
        echo && echo "running $SCRIPT_NUM tests"
        time_total_start=$(date +%s)
        for s in $SCRIPTS; do
            echo && echo && echo "========================================"
            _stop_services
            _start_services
            _run_script "${s%.sh}" "${prog}/scripts/${s}"
            if [ "$SCRIPT_EXIT" = 0 ]; then
                SUCCESSFUL_SCRIPTS+=("$s")
            else
                EXIT_CODE=1
                FAILED_SCRIPTS+=("$s")
            fi
        done
        time_total_end=$(date +%s)
        echo && echo "total test run time: $((time_total_end-time_total_start)) seconds"
        unset time_total_start time_total_end
        echo "${#SUCCESSFUL_SCRIPTS[@]} out of $SCRIPT_NUM tests were successful"
        if [ "${#FAILED_SCRIPTS[@]}" -gt 0 ]; then
            echo && echo "successful tests:"
            for s in "${SUCCESSFUL_SCRIPTS[@]}"; do
                echo "- ${s%.sh}"
            done
            echo && echo "failed tests:"
            for s in "${FAILED_SCRIPTS[@]}"; do
                echo "- ${s%.sh}"
            done
        fi
    else
        _run_script "$script_name" "$script"
            if [ "$SCRIPT_EXIT" != 0 ]; then
                EXIT_CODE=1
            fi
    fi
fi

# stop services if requested
if [ "$stop" = "1" ]; then
    if [ -n "$script" ]; then
        echo && echo "services will now be stopped"
        echo && read -rp "press <enter> to continue"
    fi
    _stop_services
    _stop_tmux
fi

# exit with 0 if all tests ran successfully
exit ${EXIT_CODE:-0}
