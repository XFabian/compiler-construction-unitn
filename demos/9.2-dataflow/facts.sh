#!/bin/bash
# Writes the facts listings of the demo to files, one per function and
# analysis, in demos/9.2-dataflow/<program>_debug/.
# Run from the repository root: bash demos/9.2-dataflow/facts.sh
set -e
DEMO=demos/9.2-dataflow
for run in 01_gen_kill:reaching 02_running:reaching 02_running:liveness \
           03_straight_live:liveness 04_appel:liveness; do
    prog=${run%%:*}
    kind=${run#*:}
    cargo run -q --bin rwetac -- --facts $kind --dump $DEMO/$prog.eta
done
ls $DEMO/*_debug/*.facts | grep -v _main.
