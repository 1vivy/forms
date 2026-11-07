# The gates of the workspace. `make check` runs the host gate and
# `make check-uefi` runs the lint of the firmware targets.

CARGO ?= cargo

.PHONY: check check-uefi test

check:
	$(CARGO) fmt --check
	$(CARGO) clippy --workspace --all-targets -- -D warnings
	$(CARGO) test --workspace
	RUSTDOCFLAGS="-D warnings" $(CARGO) doc --workspace --no-deps

check-uefi:
	$(CARGO) check-uefi-aarch64
	$(CARGO) check-uefi-x86

test:
	$(CARGO) test --workspace
