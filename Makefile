.PHONY: all build release run test deb clean

all: build

build:
	cargo build --offline

release:
	cargo build --release --offline

run:
	cargo run --offline

test:
	cargo test --offline

deb:
	./packaging/build_deb.sh

clean:
	cargo clean
	rm -rf packaging/staging *.deb
