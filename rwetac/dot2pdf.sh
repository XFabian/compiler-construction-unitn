#!/bin/bash
# Convert all .dot files in the current folder to PDFs

for file in *.dot; do
    [ -e "$file" ] || continue  # skip if no .dot files
    base="${file%.dot}"
    echo "Converting $file -> $base.pdf"
    dot -Tpdf "$file" -o "$base.pdf"
done
