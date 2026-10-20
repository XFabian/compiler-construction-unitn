#!/bin/bash
# Draws the raw and the compact CFG of all three programs as .svg files.
# Run from the repository root: bash demos/9.1-cfg/render.sh
set -e
DEMO=demos/9.1-cfg
for pair in 01_straight:f 02_max:max 03_func:func; do
    prog=${pair%%:*}
    func=${pair#*:}
    D=$DEMO/${prog}_debug
    cargo run -q --bin rwetac -- --opt --dump $DEMO/$prog.eta
    python3 $DEMO/compact_cfg.py $D/Dot_files/${prog}_${func}_initial.dot \
        $D/${prog}_compact.dot
    dot -Tsvg $D/Dot_files/${prog}_${func}_initial.dot -o $D/$prog.svg
    dot -Tsvg $D/${prog}_compact.dot -o $D/${prog}_compact.svg
    echo "$D/$prog.svg  $D/${prog}_compact.svg"
done
