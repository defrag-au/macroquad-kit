# linux-check

Runs `macroquad-storybook`'s visual test inside a Linux container, with no
display server, so the goldens for either Linux architecture can be checked or
written. It is the Linux counterpart of a bless on macOS, and the recipe is the
same shape as `shared-crates/.stacks/linux-check`.

On Apple Silicon the default container is arm64, so it writes
`tests/goldens-aarch64/<slug>/linux.png`. The amd64 image under Rosetta writes
`tests/goldens-x86_64/…` and reproduces the CI set: when that path was added,
all 37 committed x86_64 goldens matched byte for byte. CI's `ubuntu-24.04`
runner remains the check of record; the local run is how a missing golden is
written without waiting for a `bless_goldens` dispatch.

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

## The x86_64 set

`visual-x86_64.sh` runs the same test in an amd64 `ubuntu:24.04` container under
Rosetta — the runner release CI pins, so its Mesa is the renderer these goldens
are for:

```sh
container run --rm --platform linux/amd64 --rosetta --cpus 4 --memory 8G \
    -v "$PWD:/work/macroquad-kit" -w /work/macroquad-kit \
    ubuntu:24.04 /bin/sh /work/macroquad-kit/.stacks/linux-check/visual-x86_64.sh

# append `bless` to write tests/goldens-x86_64/<slug>/linux.png
```

The first run installs Rust into the mounted `.tmp/linux-check-x86_64/rustup`,
and later runs reuse it; the cargo cache is the aarch64 run's. It is slower
than the arm64 container. Run it in check mode before a bless — the existing
set should be byte-identical — and check `git status` afterwards, so a new
story is the only thing the diff shows.

An attached `run` can print `failed to send signal … signal 28` (SIGWINCH) as
the terminal resizes. It is harmless: the test summary is the end of the output,
and a bless has written the files by then. For a detached run, `up -d` is
allowed because the service is `restart = "no"`, and `container-compose logs`
reads the result; a direct `container run -d` plus `container logs` avoids the
noise too. The first x86_64 run fetches rustup, so a reset there fails the run;
retrying is the fix.

The container writes into the repository through the mount. `.tmp/linux-check/`
holds the cargo cache and the build; it is gitignored.
