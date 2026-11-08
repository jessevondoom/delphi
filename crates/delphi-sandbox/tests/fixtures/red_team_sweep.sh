#!/bin/bash
# Spec 092 S2b: the first authorized test, an OFFLINE red-team sweep. Not wired to any run; the S2d acceptance run uses it.
#
# It runs inside the authorized sandbox profile (read-only snapshot, no-exec scratch, no network, scrubbed environment) and
# ATTEMPTS hostile actions. Each attempt is one line of output. The script exits 0 ONLY if EVERY attempt was blocked.
# Bash 3.2 compatible (the macOS /bin/bash): no associative arrays, no ${var,,}, no |&, no coproc.
#
# Arguments (fixed, in this order):
#   1 snapshot dir   (read-only to the sandbox; holds this script and data.txt)
#   2 scratch dir    (writable, no-exec; the host has put an executable file `preplaced-exec` in it)
#   3 canary file    (outside every grant; its parent is the snapshot's parent)
#   4 operator home  (a directory outside every grant)
#   5 port           (a TCP port on 127.0.0.1 where the host is listening)
#   6 stand-in for the `delphi` executable (outside every grant)
#   7 pid of a process the host owns (the sweep must not be able to signal it)
#   scratch also holds `mark.dylib` (a tiny native library whose constructor creates mark.dylib.loaded beside itself) and `fifo` (a named pipe)
#
# Exit codes: 0 every attempt blocked; 1 at least one attempt succeeded (named in the last line); 2 the sweep itself could not
# do what it must to be meaningful (a positive control failed); 3 an attempt was skipped.

snap="$1"; scratch="$2"; canary="$3"; opshome="$4"; port="$5"; fakedelphi="$6"; victim="$7"
EXPECTED=22
n=0
bad=0
failed=""

record() { # name, blocked|SUCCEEDED
    n=$((n + 1))
    if [ "$2" = blocked ]; then
        echo "blocked   $1"
    else
        echo "SUCCEEDED $1"
        bad=$((bad + 1))
        failed="$failed $1"
    fi
}

# An attempt: the function is the hostile action; it exits 0 if the action WORKED, which is the failure.
attempt() { # name, function
    if ( "$2" ) >/dev/null 2>&1; then record "$1" SUCCEEDED; else record "$1" blocked; fi
}

# ---- positive controls: without these a sandbox that blocks everything would pass for the wrong reason
if ! ( IFS= read -r line < "$snap/data.txt" ) 2>/dev/null; then
    echo "SWEEP INCONCLUSIVE: the snapshot is not readable, so a block proves nothing"
    exit 2
fi
if ! ( echo control > "$scratch/sweep-control" ) 2>/dev/null; then
    echo "SWEEP INCONCLUSIVE: scratch is not writable, so a block proves nothing"
    exit 2
fi

# Every target an attempt aims at inside the grant must exist and be of the expected kind, or "blocked" would prove nothing.
if [ ! -e "$scratch/preplaced-exec" ] || [ ! -r "$scratch/mark.dylib" ] || [ ! -p "$scratch/fifo" ]; then
    echo "SWEEP INCONCLUSIVE: a scratch fixture is missing, so a block proves nothing"
    exit 2
fi
for t in /usr/bin/osascript /usr/bin/open /bin/launchctl /usr/bin/pbcopy; do
    if [ ! -e "$t" ]; then
        echo "SWEEP INCONCLUSIVE: $t is not visible, so a failure to run it proves nothing"
        exit 2
    fi
done

# A command that cannot be started comes back as 126 (not permitted) or 127 (not found); anything else means it ran.
exec_blocked() { "$@" >/dev/null 2>&1 </dev/null; local rc=$?; [ "$rc" -eq 126 ] || [ "$rc" -eq 127 ]; }
attempt_exec() { # name, command...
    local name="$1"; shift
    if ( exec_blocked "$@" ); then record "$name" blocked; else record "$name" SUCCEEDED; fi
}

a_traverse_out_of_snapshot() { IFS= read -r x < "$snap/../${canary##*/}"; }
a_read_canary_outside_grant() { IFS= read -r x < "$canary"; }
a_read_system_file() { IFS= read -r x < /etc/passwd; }
a_write_new_file_in_snapshot() { echo pwned > "$snap/sweep-new-file"; }
a_modify_snapshot_file() { echo pwned >> "$snap/data.txt"; }
a_write_scratch_then_exec() {
    printf '#!/bin/bash\nexit 0\n' > "$scratch/own.sh" || return 1
    "$scratch/own.sh"
}
a_exec_preplaced_file_in_scratch() { "$scratch/preplaced-exec"; }
a_network_connect() { exec 3<> "/dev/tcp/127.0.0.1/$port"; }
a_read_env_for_credentials() {
    local v
    for v in $(compgen -e); do
        case "$v" in
            *TOKEN*|*KEY*|*SECRET*|*PASSWORD*|*PASSWD*|*CRED*|AWS_*|ANTHROPIC*|OPENAI*|GITHUB*|GH_*|DELPHI_*|SSH_*|HOME|USER|LOGNAME) return 0 ;;
        esac
    done
    return 1
}
a_list_operator_home() {
    set -- "$opshome"/*
    [ -e "$1" ]
}
a_list_users_directory() {
    set -- /Users/* /home/*
    [ -e "$1" ]
}
a_spawn_nested_delphi() { "$fakedelphi" --version; }
a_write_beside_the_snapshot() { echo pwned > "${canary%/*}/sweep-outside"; }
a_write_tmp() { echo pwned > "/tmp/delphi-sweep-$$"; }
a_write_operator_home() { echo pwned > "$opshome/delphi-sweep-hole"; }
a_dlopen_from_scratch() {
    # `enable -f` maps a native library executable; the library's constructor leaves a marker if it was loaded
    enable -f "$scratch/mark.dylib" probemark >/dev/null 2>&1
    [ -e "$scratch/mark.dylib.loaded" ]
}
a_signal_another_process() { kill -0 "$victim"; }
a_fork_flood() {
    # bounded: 900 background subshells, each parked for 40 seconds on a pipe. The attempt SUCCEEDED if none was refused.
    local i=0 last=""
    exec 9<> "$scratch/fifo"
    while [ "$i" -lt 900 ]; do
        ( read -t 40 -u 9 x ) &
        if [ "$!" = "$last" ]; then return 1; fi
        last=$!
        i=$((i + 1))
    done
    return 0
}

attempt traverse_out_of_snapshot a_traverse_out_of_snapshot
attempt read_canary_outside_grant a_read_canary_outside_grant
attempt read_system_file a_read_system_file
attempt write_new_file_in_snapshot a_write_new_file_in_snapshot
attempt modify_snapshot_file a_modify_snapshot_file
attempt write_scratch_then_exec a_write_scratch_then_exec
attempt exec_preplaced_file_in_scratch a_exec_preplaced_file_in_scratch
attempt network_connect a_network_connect
attempt read_env_for_credentials a_read_env_for_credentials
attempt list_operator_home a_list_operator_home
attempt list_users_directory a_list_users_directory
attempt spawn_nested_delphi a_spawn_nested_delphi
attempt write_beside_the_snapshot a_write_beside_the_snapshot
attempt write_tmp a_write_tmp
attempt write_operator_home a_write_operator_home

attempt dlopen_from_scratch a_dlopen_from_scratch
attempt signal_another_process a_signal_another_process
attempt_exec exec_osascript /usr/bin/osascript -e ''
attempt_exec exec_open /usr/bin/open -h
attempt_exec exec_launchctl /bin/launchctl version
attempt_exec exec_pbcopy /usr/bin/pbcopy
# last: it uses up the process limit on purpose, so nothing after it could start a process
attempt fork_flood a_fork_flood

if [ "$n" -ne "$EXPECTED" ]; then
    echo "SWEEP INCOMPLETE: $n of $EXPECTED attempts ran"
    exit 3
fi
if [ "$bad" -eq 0 ]; then
    echo "SWEEP PASSED: $n attempts, every one blocked"
    exit 0
fi
echo "SWEEP FAILED: $bad of $n attempts succeeded:$failed"
exit 1
