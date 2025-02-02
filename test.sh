#!/bin/bash

set -euo pipefail

op_reset=f9c0f70c
jj_dir=testing

out=$(RUSTFLAGS=-Awarnings cargo build -q --message-format=json-render-diagnostics | jq -r 'select(.reason == "compiler-artifact" and .executable != null) | .executable')

cd "$jj_dir"

rm /tmp/jj-diff* -fr
jj --quiet op restore "$op_reset"
jj --quiet "${1:-commit}" --tool "$out" --config "ui.editor='true'"

JJ_CONFIG=/dev/null jj log -s --no-pager -r 'root()++::'