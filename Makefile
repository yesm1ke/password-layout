CXX ?= g++
CXXFLAGS ?= -O2
CXXFLAGS += -std=c++20 -Wall -Wextra -Isrc

# Packaging: make DESTDIR=... PREFIX=/usr install
PREFIX ?= /usr/local
BINDIR := $(PREFIX)/bin
UNIT_DIR := $(PREFIX)/lib/systemd/user
LICENSE_DIR := $(PREFIX)/share/licenses/password-layout

# A copy for the current user only, no root needed: make install-user
USER_BINDIR := $(HOME)/.local/bin
USER_UNIT_DIR := $(HOME)/.config/systemd/user

UNIT := password-layout-tty.service

VERSION := 0.2.0

# The fcitx5 addon (password fields in graphical applications) is built when
# fcitx5's development files are present, and skipped otherwise.
FCITX_VERSION := $(shell pkg-config --modversion Fcitx5Core 2>/dev/null)
ADDON := $(if $(FCITX_VERSION),build/libpasswordlayout.so)
ADDON_LIBDIR := $(PREFIX)/lib/fcitx5
ADDON_CONFDIR := $(PREFIX)/share/fcitx5/addon
USER_ADDON_LIBDIR := $(HOME)/.local/lib/password-layout
USER_ADDON_CONFDIR := $(HOME)/.local/share/fcitx5/addon

LIB_SOURCES := src/activity.cpp src/compositor.cpp src/state.cpp src/tty.cpp
HEADERS := $(wildcard src/*.h)

.PHONY: all test bench install uninstall install-user uninstall-user clean

all: build/password-layout $(ADDON)

build/password-layout: src/main.cpp $(LIB_SOURCES) $(HEADERS)
	@mkdir -p build
	$(CXX) $(CPPFLAGS) $(CXXFLAGS) $(LDFLAGS) -o $@ src/main.cpp $(LIB_SOURCES)

build/libpasswordlayout.so: src/fcitx/passwordlayout.cpp src/compositor.cpp src/state.cpp $(HEADERS)
	@mkdir -p build
	$(CXX) $(CPPFLAGS) $(CXXFLAGS) $(shell pkg-config --cflags Fcitx5Core) -fPIC -shared $(LDFLAGS) \
		-o $@ src/fcitx/passwordlayout.cpp src/compositor.cpp src/state.cpp \
		$(shell pkg-config --libs Fcitx5Core)

# fcitx5 finds the library by the name in Library=: a bare name is looked up in
# its own addon directory, an absolute path is used as is.
build/system/passwordlayout.conf build/user/passwordlayout.conf: src/fcitx/passwordlayout.conf.in
	@mkdir -p $(dir $@)
	sed -e 's|@VERSION@|$(VERSION)|' -e 's|@FCITX_VERSION@|$(FCITX_VERSION)|' \
		-e 's|@LIBRARY@|$(if $(findstring user,$@),$(USER_ADDON_LIBDIR)/libpasswordlayout,libpasswordlayout)|' \
		$< > $@

build/tests: tests/test.cpp $(LIB_SOURCES) $(HEADERS)
	@mkdir -p build
	$(CXX) $(CPPFLAGS) $(CXXFLAGS) $(LDFLAGS) -o $@ tests/test.cpp $(LIB_SOURCES)

test: build/tests
	./build/tests

# Figures and method are in BENCHMARKS.md. The loop benchmark and the service
# measurement run side by side so both see the same terminal load.
bench: build/bench-loop build/bench-pass
	./build/bench-pass
	./bench/service.sh 30 & ./build/bench-loop; wait

build/bench-%: bench/%.cpp $(LIB_SOURCES) $(HEADERS)
	@mkdir -p build
	$(CXX) $(CPPFLAGS) $(CXXFLAGS) $(LDFLAGS) -o $@ $< $(LIB_SOURCES)

# The unit has to name the binary by absolute path, which differs per install.
build/system/$(UNIT): systemd/$(UNIT).in
	@mkdir -p $(dir $@)
	sed 's|@BINDIR@|$(BINDIR)|' $< > $@

build/user/$(UNIT): systemd/$(UNIT).in
	@mkdir -p $(dir $@)
	sed 's|@BINDIR@|$(USER_BINDIR)|' $< > $@

# Installs files only; enabling the service is left to each user:
#   systemctl --user enable --now password-layout-tty.service
# fcitx5 picks the addon up on its next start.
install: all build/system/$(UNIT) $(if $(ADDON),build/system/passwordlayout.conf)
	install -Dm755 build/password-layout $(DESTDIR)$(BINDIR)/password-layout
	install -Dm644 build/system/$(UNIT) $(DESTDIR)$(UNIT_DIR)/$(UNIT)
	install -Dm644 LICENSE $(DESTDIR)$(LICENSE_DIR)/LICENSE
ifneq ($(ADDON),)
	install -Dm755 $(ADDON) $(DESTDIR)$(ADDON_LIBDIR)/libpasswordlayout.so
	install -Dm644 build/system/passwordlayout.conf $(DESTDIR)$(ADDON_CONFDIR)/passwordlayout.conf
endif

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/password-layout $(DESTDIR)$(UNIT_DIR)/$(UNIT)
	rm -f $(DESTDIR)$(ADDON_LIBDIR)/libpasswordlayout.so $(DESTDIR)$(ADDON_CONFDIR)/passwordlayout.conf
	rm -rf $(DESTDIR)$(LICENSE_DIR)

install-user: all build/user/$(UNIT) $(if $(ADDON),build/user/passwordlayout.conf)
	rm -f $(USER_BINDIR)/password-layout
	install -Dm755 build/password-layout $(USER_BINDIR)/password-layout
	install -Dm644 build/user/$(UNIT) $(USER_UNIT_DIR)/$(UNIT)
	systemctl --user daemon-reload
	systemctl --user enable $(UNIT)
	systemctl --user restart $(UNIT)
ifneq ($(ADDON),)
	rm -f $(USER_ADDON_LIBDIR)/libpasswordlayout.so
	install -Dm755 $(ADDON) $(USER_ADDON_LIBDIR)/libpasswordlayout.so
	install -Dm644 build/user/passwordlayout.conf $(USER_ADDON_CONFDIR)/passwordlayout.conf
	@echo "Restart fcitx5 to load the addon, for example: fcitx5 -rd"
endif

uninstall-user:
	-systemctl --user disable --now $(UNIT)
	rm -f $(USER_UNIT_DIR)/$(UNIT) $(USER_BINDIR)/password-layout
	rm -f $(USER_ADDON_LIBDIR)/libpasswordlayout.so $(USER_ADDON_CONFDIR)/passwordlayout.conf
	systemctl --user daemon-reload

clean:
	rm -rf build
