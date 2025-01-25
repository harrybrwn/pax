build:
	cargo build --release

install: install-types
	cargo install --path ./pax

install-types:
	mkdir -p /usr/share/LuaLS/pax/_meta/
	cp lua/_meta/pax.lua /usr/share/LuaLS/pax/_meta/

uninstall:
	cargo uninstall pax
	rm -rf /usr/share/LuaLS/pax/

.PHONY: unpack
unpack:
	@echo
