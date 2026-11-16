#!/bin/bash
# Spec 114 M7: make_home.sh <delphi binary> <meter port> <ollama model>
# A fresh scratch home (never ~/.delphi) with only the local Ollama model, its endpoint pointed at token_meter.py on <meter port>,
# a one-hour job limit for both sides alike (a slow local model passes the default 600 seconds), and its daemon started.
# `delphi setup` is driven at a pseudo-terminal: every subscription and key is declined and the scratch home's access change is
# ratified there, as an operator would at a terminal. Prints the home.
set -e
here=$(cd "$(dirname "$0")" && pwd)
bin=$1; port=$2; model=${3:-gemma4:latest}
H=$(mktemp -d /tmp/m7h.XXXX)
work=$(mktemp -d /tmp/m7cwd.XXXX)   # not a Git repository, so no project is registered
cd "$work"
"$bin" init --non-interactive --home "$H" >/dev/null 2>&1
python3 -I "$here/setup_drive.py" "$bin" "$H" "$model" "$work" | grep -q Ratified
cfg="$H/workspaces/default/config.toml"
sed -i '' "s#base_url = \"http://localhost:11434\"#base_url = \"http://127.0.0.1:$port\"#" "$cfg"
printf '\n[research.limits]\njob_secs = 3600\n' >> "$cfg"
python3 -I "$here/setup_drive.py" "$bin" "$H" "$model" "$work" | grep -q Ratified
"$bin" daemon restart --home "$H" >/dev/null
echo "$H"
