build:
	cargo build --release

install:
	cargo install --path ./pax

uninstall:
	cargo uninstall pax

.PHONY: unpack
unpack:
	@echo
