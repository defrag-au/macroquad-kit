#!/bin/sh
# Run the storybook's visual test inside the linux-check container.
#
#   container-compose -f .stacks/linux-check/stack.toml run -T check
#   container-compose -f .stacks/linux-check/stack.toml run -T check -- \
#       /bin/sh /work/macroquad-kit/.stacks/linux-check/visual-linux.sh bless
#
# No argument checks the goldens; `bless` writes them for this container's
# architecture. On Apple Silicon that is aarch64, so it writes
# `tests/goldens-aarch64/<slug>/linux.png`. The x86_64 set is CI's: run the
# `Visual Goldens` workflow with `bless_goldens`.
set -eu

apt-get update -qq
apt-get install -y -qq --no-install-recommends \
    libx11-6 libgl1 libglx-mesa0 libegl1 libgl1-mesa-dri

export CARGO_TERM_COLOR=never
export CARGO_HOME=/work/macroquad-kit/.tmp/linux-check/cargo-home
export CARGO_TARGET_DIR=/work/macroquad-kit/.tmp/linux-check/out/target
# llvmpipe is the renderer these goldens are for; there is no display server.
export LIBGL_ALWAYS_SOFTWARE=1

if [ "${1:-}" = "bless" ]; then
    export UPDATE_GOLDENS=1
fi

cd /work/macroquad-kit
cargo test -p macroquad-storybook --test visual -- --test-threads=2
