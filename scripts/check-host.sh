#!/bin/sh
set -eu

script_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
workspace_directory=$(CDPATH= cd -- "$script_directory/.." && pwd)
cd "$workspace_directory"

cargo run --quiet -p xtask -- check
