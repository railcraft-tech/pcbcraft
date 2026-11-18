# Build pcbcraft and run its unit tests.
#
# `make` compiles every crate. `make test` runs the tests that live next to
# the source (`#[cfg(test)]` in libraries and in the pcbcraft binary).

CARGO ?= cargo

.PHONY: all build test

all: build

build:
	$(CARGO) build --workspace

test:
	$(CARGO) test --workspace --lib --bins
