PREFIX ?= /usr
DESTDIR ?=
VERSION := 0.1.0
CARGO ?= cargo
BINDIR := $(DESTDIR)$(PREFIX)/bin
DATADIR := $(DESTDIR)$(PREFIX)/share
APPDIR := $(DATADIR)/applications
SESSIONDIR := $(DATADIR)/wayland-sessions
DOCDIR := $(DATADIR)/doc/kyonde
KYONDIR := $(DATADIR)/kyonde

.PHONY: all build release install uninstall config desktop-package arch-package clean run run-nested run-tty

all: build

build:
	$(CARGO) build -p kyonde -p kyonshell -p kyonctl

release:
	$(CARGO) build --release -p kyonde -p kyonshell -p kyonctl

install: release
	install -d "$(BINDIR)" "$(APPDIR)" "$(SESSIONDIR)" "$(DOCDIR)" "$(KYONDIR)"
	install -m 0755 target/release/kyonde "$(BINDIR)/kyonde"
	install -m 0755 target/release/kyonshell "$(BINDIR)/kyonshell"
	install -m 0755 target/release/kyonctl "$(BINDIR)/kyonctl"
	install -m 0755 scripts/kyon-session "$(BINDIR)/kyon-session"
	install -m 0644 examples/config.lua "$(KYONDIR)/config.lua"
	install -m 0644 LICENSE "$(DOCDIR)/LICENSE"
	install -m 0644 README.md "$(DOCDIR)/README.md"
	sed "s|@BIN_DIR@|$(PREFIX)/bin|g" data/kyonshell-settings.desktop.in > "$(APPDIR)/kyonshell-settings.desktop"
	sed "s|@BIN_DIR@|$(PREFIX)/bin|g" data/kyonshell-launcher.desktop.in > "$(APPDIR)/kyonshell-launcher.desktop"
	sed "s|@BIN_DIR@|$(PREFIX)/bin|g" data/kyonde-nested.desktop.in > "$(APPDIR)/kyonde-nested.desktop"
	install -m 0644 data/kyonde-session.desktop "$(SESSIONDIR)/kyonde.desktop"
	command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$(APPDIR)" || true

uninstall:
	rm -f "$(BINDIR)/kyonde" "$(BINDIR)/kyonshell" "$(BINDIR)/kyonctl" "$(BINDIR)/kyon-session"
	rm -f "$(APPDIR)/kyonshell-settings.desktop" "$(APPDIR)/kyonshell-launcher.desktop" "$(APPDIR)/kyonde-nested.desktop"
	rm -f "$(SESSIONDIR)/kyonde.desktop"
	rm -rf "$(DOCDIR)" "$(KYONDIR)"

run: build
	./scripts/kyon-session

run-nested: build
	PATH="$(CURDIR)/target/debug:$$PATH" ./target/debug/kyonde winit

run-tty: build
	./target/debug/kyonde tty

arch-package:
	./scripts/make-arch-package.sh

clean:
	$(CARGO) clean

dist: 
	rm -rf dist
	mkdir -p dist
	tar --exclude=target --exclude=.git --exclude=dist --exclude=smithay/.git --exclude=smithay/target -czf dist/kyonde-$(VERSION).tar.gz --transform 's,^,kyonde-$(VERSION)/,' .
