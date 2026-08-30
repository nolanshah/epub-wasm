# epub-wasm build orchestration.
#
# The ordering that matters: server-test embeds renderer/pkg at compile time,
# and client-test runs its own copy of it — so `wasm` must run before `build`,
# and the pkg copy must be re-synced after every wasm build (a stale copy
# means the demos silently run old WASM).

.PHONY: all wasm build test e2e e2e-install fixture serve clean

all: wasm build test

# Build the WASM package and sync the client's copy
wasm:
	wasm-pack build --release --target web renderer
	rm -rf client-test/pkg
	cp -r renderer/pkg client-test/pkg

# Native release build (test server + core); requires `make wasm` first
build:
	cargo build --release

# Native tests (unit + integration)
test:
	cargo test --workspace

# Browser click-path tests (requires `make wasm` first)
e2e: e2e-install
	cd e2e && npx playwright test

e2e-install:
	cd e2e && npm install --no-fund --no-audit

# Regenerate the e2e fixture EPUB after editing core/examples/make_fixture.rs
fixture:
	cargo run -p epub-reader-core --example make_fixture -- e2e/fixtures/fixture.epub

# Serve the client-side demo
serve:
	cd client-test && python3 -m http.server 8080

clean:
	cargo clean
	rm -rf renderer/pkg client-test/pkg e2e/node_modules e2e/.serve e2e/test-results
