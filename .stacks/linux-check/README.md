# linux-check

Runs `macroquad-storybook`'s visual test inside a Linux container, with no
display server, so the goldens for this machine's Linux architecture can be
checked or written. It is the Linux counterpart of a bless on macOS, and the
recipe is the same shape as `shared-crates/.stacks/linux-check`.

On Apple Silicon the container is arm64, so this writes
`tests/goldens-aarch64/<slug>/linux.png`. It **cannot** write the x86_64 set:
that is CI's, on `ubuntu-24.04`, whose Mesa is the renderer the comparison is
pinned to. A `workflow_dispatch` run of `Visual Goldens` with `bless_goldens`
writes that set and uploads it as `goldens-x86_64`.

## First run

The kit fetches its dependencies by git rev, and the private ones need
credentials the container does not have. Seed the cache from the host, where the
checkout already works:

```sh
CARGO_HOME=.tmp/linux-check/cargo-home cargo fetch
```

`container-compose` is on `PATH` in a shell with defrag-nix's container tooling
(shared-crates' devshell has it); the kit's devshell does not, so run it from one
that does, with the stack path as written.

## Check or bless

```sh
# check the committed Linux goldens against this container's renderer
container-compose -f .stacks/linux-check/stack.toml run -T check

# write tests/goldens-<arch>/<slug>/linux.png for every selected story
container-compose -f .stacks/linux-check/stack.toml run -T check -- \
    /bin/sh /work/macroquad-kit/.stacks/linux-check/visual-linux.sh bless
```

A bless rewrites every selected story's golden, so run it on a clean tree and
read the diff: existing goldens should be byte-identical, and only a new story
should appear.

An attached `run` can print `failed to send signal … signal 28` (SIGWINCH) as
the terminal resizes. It is harmless: the test summary is the end of the output,
and a bless has written the files by then. For a detached run, `up -d` is
allowed because the service is `restart = "no"`, and `container-compose logs`
reads the result.

The container writes into the repository through the mount. `.tmp/linux-check/`
holds the cargo cache and the build; it is gitignored.
