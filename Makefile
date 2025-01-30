build:
	cargo build --release

install: install-types
	cargo install --path ./pax

install-types:
	if [ ! -d /usr/share/LuaLS/pax/_meta ]; then \
		sudo mkdir --mode=0777 -p /usr/share/LuaLS/pax/_meta/ ; \
	fi
	cp lua/_meta/pax.lua /usr/share/LuaLS/pax/_meta/

uninstall:
	cargo uninstall pax
	rm -rf /usr/share/LuaLS/pax/

.PHONY: images
images:
	docker buildx bake pax

.PHONY: unpack
unpack:
	@echo
