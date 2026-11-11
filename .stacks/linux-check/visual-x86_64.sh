#!/bin/sh
# Run the storybook's visual test as x86_64 Linux, under Rosetta, so the
# x86_64 goldens can be written on an Apple Silicon machine.
#
#   container run --rm --platform linux/amd64 --rosetta --cpus 4 --memory 8G \
#       -v "$PWD:/work/macroquad-kit" -w /work/macroquad-kit \
#       ubuntu:24.04 /bin/sh /work/macroquad-kit/.stacks/linux-check/visual-x86_64.sh
#
# With `bless` it writes `tests/goldens-x86_64/<slug>/linux.png`.
#
# `ubuntu:24.04` is the CI runner's release, and its Mesa is the x86_64
# contract. The container is emulated, but llvmpipe computes the same pixels:
# when this path was added, every committed golden matched byte for byte. CI
# remains the check of record — this is how a missing golden gets written
# without waiting for a dispatch.
#
# Rust comes from rustup on the first run, into the mounted `.tmp/`; the
# cargo cache is the aarch64 run's, seeded from the host (see README.md).
set -eu

apt-get update -qq
apt-get install -y -qq --no-install-recommends \
    curl ca-certificates build-essential pkg-config \
    libx11-6 libgl1 libglx-mesa0 libegl1 libgl1-mesa-dri

export RUSTUP_HOME=/work/macroquad-kit/.tmp/linux-check-x86_64/rustup
export CARGO_HOME=/work/macroquad-kit/.tmp/linux-check/cargo-home
if [ ! -x "$CARGO_HOME/bin/rustup" ]; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --profile minimal --default-toolchain stable
fi
. "$CARGO_HOME/env"

export CARGO_TERM_COLOR=never
export CARGO_TARGET_DIR=/work/macroquad-kit/.tmp/linux-check-x86_64/target
# llvmpipe is the renderer these goldens are for; there is no display server.
# The headless path loads EGL (libEGL_mesa, libgallium, libgbm), so libegl1 is
# the package that matters; LIBGL_ALWAYS_SOFTWARE changes nothing here.
export LIBGL_ALWAYS_SOFTWARE=1

if [ "${1:-}" = "bless" ]; then
    export UPDATE_GOLDENS=1
fi

cd /work/macroquad-kit
cargo test -p macroquad-storybook --test visual -- --test-threads=2
