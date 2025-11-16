#!/usr/bin/env bash
# Source this helper to configure the local Python/OCaml native FFI environment.
set -euo pipefail

PYENV_PY_DEFAULT="$HOME/.pyenv/versions/3.11.10"
PYENV_PY="${PYENV_PY:-$PYENV_PY_DEFAULT}"
if [ ! -d "$PYENV_PY" ]; then
  echo "warning: PYENV_PY ($PYENV_PY) does not exist" >&2
fi

export PYENV_PY
export PYO3_PYTHON="$PYENV_PY/bin/python3.11"
export RUSTFLAGS="-C link-arg=-L${PYENV_PY}/lib -C link-arg=-lpython3.11 -C link-arg=-ldl -C link-arg=-lm -C link-arg=-lpthread -C link-arg=-lutil -C link-arg=-Wl,-rpath,${PYENV_PY}/lib"

if command -v opam >/dev/null 2>&1; then
  eval "$(opam env --set-switch --switch=default 2>/dev/null || opam env 2>/dev/null)"
else
  echo "warning: opam not found; skip OCaml env" >&2
fi
