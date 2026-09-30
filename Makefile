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

LIB_SOURCES := src/activity.cpp src/compositor.cpp src/state.cpp src/tty.cpp
HEADERS := $(wildcard src/*.h)

.PHONY: all test bench install uninstall install-user uninstall-user clean

all: build/password-layout

build/password-layout: src/main.cpp $(LIB_SOURCES) $(HEADERS)
	@mkdir -p build
	$(CXX) $(CPPFLAGS) $(CXXFLAGS) $(LDFLAGS) -o $@ src/main.cpp $(LIB_SOURCES)

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
install: build/password-layout build/system/$(UNIT)
	install -Dm755 build/password-layout $(DESTDIR)$(BINDIR)/password-layout
	install -Dm644 build/system/$(UNIT) $(DESTDIR)$(UNIT_DIR)/$(UNIT)
	install -Dm644 LICENSE $(DESTDIR)$(LICENSE_DIR)/LICENSE

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/password-layout $(DESTDIR)$(UNIT_DIR)/$(UNIT)
	rm -rf $(DESTDIR)$(LICENSE_DIR)

install-user: build/password-layout build/user/$(UNIT)
	rm -f $(USER_BINDIR)/password-layout
	install -Dm755 build/password-layout $(USER_BINDIR)/password-layout
	install -Dm644 build/user/$(UNIT) $(USER_UNIT_DIR)/$(UNIT)
	systemctl --user daemon-reload
	systemctl --user enable $(UNIT)
	systemctl --user restart $(UNIT)

uninstall-user:
	-systemctl --user disable --now $(UNIT)
	rm -f $(USER_UNIT_DIR)/$(UNIT) $(USER_BINDIR)/password-layout
	systemctl --user daemon-reload

clean:
	rm -rf build
