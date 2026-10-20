#!/bin/bash
# Builds the Wasmtime explorer pages for 02_example and 03_pressure, once per
# register allocator: WAT, Cranelift's IR and x86 side by side.
# Run from the repository root: bash demos/12.1-regalloc/explore.sh
set -e
DEMO=demos/12.1-regalloc
OUT=$DEMO/explore_debug
WASMTIME=${WASMTIME:-$(command -v wasmtime || echo ~/.wasmtime/bin/wasmtime)}
mkdir -p $OUT
for prog in 02_example 03_pressure; do
    cargo run -q --bin rwetac -- $DEMO/$prog.eta
    for alloc in backtracking single-pass; do
        page=$OUT/${prog}_$alloc.html
        clif=$OUT/${prog}_${alloc}_clif
        rm -rf $clif
        $WASMTIME explore -O opt-level=0 -O regalloc-algorithm=$alloc \
            $DEMO/$prog.wat -o $page > /dev/null
        $WASMTIME compile -O opt-level=0 -O regalloc-algorithm=$alloc \
            --emit-clif $clif $DEMO/$prog.wat -o $OUT/$prog.cwasm
        python3 $DEMO/add_clif.py $page $clif
        echo $page
    done
done
