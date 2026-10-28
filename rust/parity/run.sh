#!/usr/bin/env bash
# Stage 1-3 parity check: the C++ (src/state.cpp, src/compositor.cpp)
# against the Rust library (password-layout-core), driven the same way by
# rust/parity/parity_driver.cpp and rust/password-layout-core/examples/
# parity_driver.rs (see the wiki page "Rust rewrite", "Parity check").
#
#   rust/parity/run.sh [extra-scenarios-file]
#
# For every classify line (tests/xkb-latin.tsv plus rust/parity/
# classify-extra.txt) and every scenario (rust/parity/scenarios.txt plus the
# optional extra file) both sides are run in fresh directories and stdout,
# stderr and the directory contents (file names and bytes) are compared.
# Every JSON fixture (rust/parity/json/*.json, the keys on its first line
# "# keys: K1 K2 ...") is run through both drivers' `json` subcommand and
# stdout is compared (see the wiki page "Rust rewrite", "Parity check").
#
# Hyprland cases (rust/parity/hyprland/<case>/ with devices.json,
# activewindow.json and ops, plus one no-server case): both drivers run
# `hyprland ...` against a Rust fake Hyprland (the parity driver's `serve`,
# Rust only: C++ has no fake) in a fresh runtime directory each, and stdout,
# stderr and the request logs are compared. The fake's stdin is a fifo the
# script holds open until the driver has finished, so the fake can print its
# requests afterwards. Every driver run and wait is bounded by `timeout 20`;
# a hang is a difference, not a lockup.
#
# Doctor cases (rust/parity/doctor/<case>/): both binaries run `doctor`
# under `env -i` and `timeout 20` with only the case's PATH stubs
# (<command>.out is printed by a stub for systemctl, fcitx5, pacman or
# sudo; <command>.code is its exit number, default 0), a fresh HOME,
# XDG_CONFIG_HOME (with profile if present) and XDG_RUNTIME_DIR, and
# (unless the case has no-signature) HYPRLAND_INSTANCE_SIGNATURE=parity.
# With devices.json, a fake Hyprland (parity_driver serve) answers; with
# fcitx5-running, a background process named fcitx5 is alive for the /proc
# scan. stdout and the exit code must match.
#
# Prints "parity: N classify lines, M scenarios, J json files, H hyprland
# cases, K doctor cases, D differences"; exits 1 when D > 0. Works from any
# current directory; its temp dirs are cleaned up.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
workdir=$(mktemp -d)
trap 'rm -rf "$workdir"' EXIT

parity_build="$root/rust/target/parity"
mkdir -p "$parity_build"
cpp_driver="$parity_build/parity_driver"
rust_driver="${CARGO_TARGET_DIR:-$root/rust/target}/debug/examples/parity_driver"

# 1. Build both drivers.
g++ -std=c++20 -I"$root/src" -o "$cpp_driver" \
    "$here/parity_driver.cpp" "$root/src/state.cpp" "$root/src/compositor.cpp"
(cd "$root/rust" && cargo build --example parity_driver)

# 2. Classify input: every table row "x(y)<TAB>n" becomes "x<TAB>y".
classify_in="$workdir/classify.txt"
: > "$classify_in"
while IFS= read -r line; do
    [[ -z "$line" ]] && continue
    name=${line%%$'\t'*}
    open=${name%%(*}
    variant=""
    if [[ "$name" == *"("*")" ]]; then
        variant=${name#"$open("}
        variant=${variant%")"}
    fi
    printf '%s\t%s\n' "$open" "$variant" >> "$classify_in"
done < "$root/tests/xkb-latin.tsv"
while IFS= read -r line; do
    [[ -z "$line" || "$line" == \#* ]] && continue
    printf '%s\n' "$line" >> "$classify_in"
done < "$here/classify-extra.txt"
classify_lines=$(wc -l < "$classify_in")

differences=0
diff_file="$workdir/diff.txt"
check() {
    local what=$1 a=$2 b=$3
    if ! diff -u "$a" "$b" > "$diff_file"; then
        differences=$((differences + 1))
        echo "parity difference: $what"
        head -n 40 "$diff_file"
    fi
}

"$cpp_driver" classify < "$classify_in" > "$workdir/classify_cpp.txt" 2> "$workdir/classify_cpp.err"
"$rust_driver" classify < "$classify_in" > "$workdir/classify_rust.txt" 2> "$workdir/classify_rust.err"
check "classify stdout" "$workdir/classify_cpp.txt" "$workdir/classify_rust.txt"
check "classify stderr" "$workdir/classify_cpp.err" "$workdir/classify_rust.err"

# 3. Scenarios: "INDEX LATIN OP..." (LATIN may be empty, hence the manual
# field splitting: `read` would not keep the empty field).
digest() {
    (cd "$1" && find . -type f -exec md5sum {} + | sort)
}

scenario_files=("$here/scenarios.txt")
if [[ $# -ge 1 ]]; then
    scenario_files+=("$1")
fi
scenarios=0
for scenario_file in "${scenario_files[@]}"; do
    while IFS= read -r line || [[ -n "$line" ]]; do
        line=${line%$'\r'}
        [[ -z "$line" || "$line" == \#* ]] && continue
        index=${line%% *}
        rest=${line#"$index "}
        rest=${rest#" "}
        if [[ "$rest" == " "* ]]; then
            latin=""
            ops_part=${rest# }
        else
            latin=${rest%% *}
            if [[ "$rest" == *" "* ]]; then
                ops_part=${rest#"$latin "}
            else
                ops_part=""
            fi
        fi
        ops=()
        if [[ -n "$ops_part" ]]; then
            read -r -a ops <<< "$ops_part"
        fi
        scenarios=$((scenarios + 1))
        cpp_dir=$(mktemp -d "$workdir/cpp.XXXXXX")
        rust_dir=$(mktemp -d "$workdir/rust.XXXXXX")
        cpp_status=0
        "$cpp_driver" run "$cpp_dir" "$index" "$latin" ${ops[@]+"${ops[@]}"} \
            > "$workdir/cpp_out.txt" 2> "$workdir/cpp_err.txt" || cpp_status=$?
        rust_status=0
        "$rust_driver" run "$rust_dir" "$index" "$latin" ${ops[@]+"${ops[@]}"} \
            > "$workdir/rust_out.txt" 2> "$workdir/rust_err.txt" || rust_status=$?
        echo "$cpp_status" > "$workdir/cpp_status.txt"
        echo "$rust_status" > "$workdir/rust_status.txt"
        digest "$cpp_dir" > "$workdir/cpp_dir.txt"
        digest "$rust_dir" > "$workdir/rust_dir.txt"
        check "scenario \"$line\" stdout" "$workdir/cpp_out.txt" "$workdir/rust_out.txt"
        check "scenario \"$line\" stderr" "$workdir/cpp_err.txt" "$workdir/rust_err.txt"
        check "scenario \"$line\" exit status" "$workdir/cpp_status.txt" "$workdir/rust_status.txt"
        check "scenario \"$line\" directory" "$workdir/cpp_dir.txt" "$workdir/rust_dir.txt"
    done < "$scenario_file"
done

# 4. JSON fixtures: rust/parity/json/*.json; the first line "# keys: K1 K2
# ..." names the keys (it is part of the input) and the driver's stdout must
# match byte for byte.
json_files=0
for json_file in "$here"/json/*.json; do
    [[ -f "$json_file" ]] || continue
    json_files=$((json_files + 1))
    name=$(basename "$json_file")
    keys_line=$(head -n 1 "$json_file")
    keys=()
    read -r -a keys <<< "${keys_line#*# keys: }"
    timeout 20 "$cpp_driver" json ${keys[@]+"${keys[@]}"} < "$json_file" \
        > "$workdir/json_cpp.txt" 2> "$workdir/json_cpp.err"
    timeout 20 "$rust_driver" json ${keys[@]+"${keys[@]}"} < "$json_file" \
        > "$workdir/json_rust.txt" 2> "$workdir/json_rust.err"
    check "json $name stdout" "$workdir/json_cpp.txt" "$workdir/json_rust.txt"
done

# 5. Hyprland cases: rust/parity/hyprland/*/ with devices.json,
# activewindow.json and ops. The fake is Rust-only, so both sides run
# against it; each side gets a fresh runtime directory and the request logs
# must match as well.
run_hyprland_side() {
    local side=$1 case_dir=$2 driver=$3
    local runtime fifo socket serve_pid
    runtime=$(mktemp -d "$workdir/hyp-$side.XXXXXX")
    fifo="$workdir/hyp-$side.fifo"
    rm -f "$fifo"
    mkfifo "$fifo"
    "$rust_driver" serve "$runtime" "$signature" "$case_dir/devices.json" "$case_dir/activewindow.json" \
        < "$fifo" > "$workdir/hyp-$side.requests" 2> "$workdir/hyp-$side.serve.err" &
    serve_pid=$!
    # Open the fifo read-write: a write-only open would block until the
    # fake comes up (a dead fake would hang the script), and the fake
    # unblocks its own stdin open as soon as any end is open.
    exec 3<>"$fifo"
    socket="$runtime/hypr/$signature/.socket.sock"
    for _ in $(seq 1 50); do
        [[ -S "$socket" ]] && break
        kill -0 "$serve_pid" 2>/dev/null || break
        sleep 0.1
    done
    if [[ ! -S "$socket" ]]; then
        # Kill the fake (closing the fifo does not unblock a blocked open)
        # and make all the compared files exist.
        exec 3>&-
        kill "$serve_pid" 2>/dev/null || true
        wait "$serve_pid" 2>/dev/null || true
        echo "hyprland case (serve $side): the fake did not come up" >&2
        : > "$workdir/hyp-$side.out"
        : > "$workdir/hyp-$side.err"
        : > "$workdir/hyp-$side.requests"
        : > "$workdir/hyp-$side.serve.err"
    else
        timeout 20 "$driver" hyprland "$runtime" "$signature" "${ops[@]}" \
            > "$workdir/hyp-$side.out" 2> "$workdir/hyp-$side.err" || true
        exec 3>&-
        # The fifo just reached EOF; the fake prints its requests and exits.
        if ! timeout 20 tail --pid="$serve_pid" -f /dev/null 2>/dev/null; then
            kill "$serve_pid" 2>/dev/null || true
            echo "hyprland case (serve $side): the fake did not exit in 20 s" >&2
        fi
        wait "$serve_pid" 2>/dev/null || true
    fi
}

signature="parity"
hyprland_cases=0
for case_dir in "$here"/hyprland/*/; do
    [[ -f "$case_dir/devices.json" && -f "$case_dir/activewindow.json" && -f "$case_dir/ops" ]] || continue
    hyprland_cases=$((hyprland_cases + 1))
    name=$(basename "$case_dir")
    read -r -a ops < "$case_dir/ops"
    run_hyprland_side cpp "$case_dir" "$cpp_driver"
    run_hyprland_side rust "$case_dir" "$rust_driver"
    check "hyprland case $name stdout" "$workdir/hyp-cpp.out" "$workdir/hyp-rust.out"
    check "hyprland case $name stderr" "$workdir/hyp-cpp.err" "$workdir/hyp-rust.err"
    check "hyprland case $name requests" "$workdir/hyp-cpp.requests" "$workdir/hyp-rust.requests"
    check "hyprland case $name serve stderr" "$workdir/hyp-cpp.serve.err" "$workdir/hyp-rust.serve.err"
done

# 5b. Without a server: both sides must fail every OP the same way.
hyprland_cases=$((hyprland_cases + 1))
no_server_runtime=$(mktemp -d "$workdir/hyp-noserver.XXXXXX")
timeout 20 "$cpp_driver" hyprland "$no_server_runtime" "$signature" \
    index latin list focused set:0 \
    > "$workdir/noserver_cpp.out" 2> "$workdir/noserver_cpp.err" || true
timeout 20 "$rust_driver" hyprland "$no_server_runtime" "$signature" \
    index latin list focused set:0 \
    > "$workdir/noserver_rust.out" 2> "$workdir/noserver_rust.err" || true
check "hyprland case no-server stdout" "$workdir/noserver_cpp.out" "$workdir/noserver_rust.out"
check "hyprland case no-server stderr" "$workdir/noserver_cpp.err" "$workdir/noserver_rust.err"

# 6. Doctor cases: rust/parity/doctor/*/. Both binaries run the real
# `doctor` subcommand in the stub environment described in the header
# comment; stdout and the exit code are compared.
rust_bin="${CARGO_TARGET_DIR:-$root/rust/target}/release/password-layout"
(cd "$root/rust" && cargo build --release -p password-layout)
g++ -std=c++20 -O2 -I"$root/src" -o "$workdir/cpp-password-layout" \
    "$root/src/main.cpp" "$root/src/activity.cpp" "$root/src/compositor.cpp" \
    "$root/src/doctor.cpp" "$root/src/state.cpp" "$root/src/tmux.cpp" \
    "$root/src/tty.cpp" "$root/src/watch.cpp"

run_doctor_side() {
    # $1 side (cpp/rust)  $2 case dir  $3 binary  $4 stdout file  $5 status file
    local side=$1 case_dir=$2 binary=$3 out=$4 status_file=$5
    local stubs runtime config home cmd code
    stubs=$(mktemp -d "$workdir/doc-${side}-stub.XXXXXX")
    runtime=$(mktemp -d "$workdir/doc-${side}-rt.XXXXXX")
    config=$(mktemp -d "$workdir/doc-${side}-cfg.XXXXXX")
    home=$(mktemp -d "$workdir/doc-${side}-home.XXXXXX")
    for cmd in systemctl fcitx5 pacman sudo; do
        if [[ -f "$case_dir/$cmd.out" ]]; then
            code=0
            if [[ -f "$case_dir/$cmd.code" ]]; then
                code=$(<"$case_dir/$cmd.code")
            fi
            {
                printf '#!/bin/sh\n'
                printf 'while IFS= read -r l; do printf "%%s\\n" "$l"; done < "%s"\n' \
                    "$case_dir/$cmd.out"
                printf 'exit %s\n' "$code"
            } > "$stubs/$cmd"
            chmod +x "$stubs/$cmd"
        fi
    done
    if [[ -f "$case_dir/profile" ]]; then
        mkdir -p "$config/fcitx5"
        cp "$case_dir/profile" "$config/fcitx5/profile"
    fi
    local fake_fcitx= fifo= serve_pid= window= socket=
    if [[ -f "$case_dir/fcitx5-running" ]]; then
        # A live process named fcitx5, for the /proc scan of running input
        # methods: /proc/<pid>/comm is the executable's name, so a copy of
        # sleep named fcitx5 is enough. The copy must not sit in $stubs (the
        # stubs dir is the whole PATH, and a stub named fcitx5 would answer
        # `fcitx5 --version`), and the file name must be exactly fcitx5, so
        # the per-side prefix goes in the directory, not the name.
        mkdir -p "$workdir/doc-${side}-fake"
        cp /bin/sleep "$workdir/doc-${side}-fake/fcitx5"
        "$workdir/doc-${side}-fake/fcitx5" 30 &
        fake_fcitx=$!
    fi
    if [[ -f "$case_dir/devices.json" ]]; then
        if [[ -f "$case_dir/activewindow.json" ]]; then
            window="$case_dir/activewindow.json"
        else
            window="$workdir/doc-${side}-window.json"
            printf '{}' > "$window"
        fi
        fifo="$workdir/doc-${side}.fifo"
        rm -f "$fifo"
        mkfifo "$fifo"
        "$rust_driver" serve "$runtime" "$signature" "$case_dir/devices.json" "$window" \
            < "$fifo" > "$workdir/doc-${side}.requests" \
            2> "$workdir/doc-${side}.serve.err" &
        serve_pid=$!
        # Open the fifo read-write, as in run_hyprland_side.
        exec 3<>"$fifo"
        socket="$runtime/hypr/$signature/.socket.sock"
        for _ in $(seq 1 50); do
            [[ -S "$socket" ]] && break
            kill -0 "$serve_pid" 2>/dev/null || break
            sleep 0.1
        done
        if [[ ! -S "$socket" ]]; then
            echo "doctor case (serve $side): the fake did not come up" >&2
            exec 3>&-
            kill "$serve_pid" 2>/dev/null || true
            wait "$serve_pid" 2>/dev/null || true
        fi
    fi
    local -a envargs=(
        PATH="$stubs"
        XDG_RUNTIME_DIR="$runtime"
        XDG_CONFIG_HOME="$config"
        HOME="$home"
    )
    if [[ ! -f "$case_dir/no-signature" ]]; then
        envargs+=("HYPRLAND_INSTANCE_SIGNATURE=$signature")
    fi
    local status=0
    timeout 20 env -i "${envargs[@]}" "$binary" doctor > "$out" \
        2> "$out.err" || status=$?
    echo "$status" > "$status_file"
    if [[ -n "$serve_pid" ]]; then
        # Close the fifo: the fake's stdin read ends and it exits.
        exec 3>&-
        if ! timeout 20 tail --pid="$serve_pid" -f /dev/null 2>/dev/null; then
            kill "$serve_pid" 2>/dev/null || true
            echo "doctor case (serve $side): the fake did not exit in 20 s" >&2
        fi
        wait "$serve_pid" 2>/dev/null || true
    fi
    if [[ -n "$fake_fcitx" ]]; then
        kill "$fake_fcitx" 2>/dev/null || true
        wait "$fake_fcitx" 2>/dev/null || true
    fi
    return 0
}

doctor_cases=0
for case_dir in "$here"/doctor/*/; do
    [[ -d "$case_dir" ]] || continue
    doctor_cases=$((doctor_cases + 1))
    name=$(basename "$case_dir")
    run_doctor_side cpp "$case_dir" "$workdir/cpp-password-layout" \
        "$workdir/doc-cpp.out" "$workdir/doc-cpp.status"
    run_doctor_side rust "$case_dir" "$rust_bin" \
        "$workdir/doc-rust.out" "$workdir/doc-rust.status"
    check "doctor case $name stdout" "$workdir/doc-cpp.out" "$workdir/doc-rust.out"
    check "doctor case $name exit code" \
        "$workdir/doc-cpp.status" "$workdir/doc-rust.status"
done

echo "parity: $classify_lines classify lines, $scenarios scenarios, $json_files json files, $hyprland_cases hyprland cases, $doctor_cases doctor cases, $differences differences"
[[ "$differences" -eq 0 ]]
