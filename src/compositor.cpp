#include "compositor.h"

#include <cerrno>
#include <charconv>
#include <cstdlib>
#include <map>
#include <stdexcept>
#include <sys/socket.h>
#include <sys/un.h>
#include <system_error>
#include <unistd.h>

namespace {

std::string requireEnv(const char *name) {
    const char *value = std::getenv(name);
    if (!value || !*value) {
        throw std::runtime_error(std::string(name) + " is not set");
    }
    return value;
}

[[noreturn]] void fail(const char *what) {
    throw std::system_error(errno, std::generic_category(), what);
}

} // namespace

std::vector<long> jsonIntegers(std::string_view json, std::string_view key) {
    std::string needle = "\"" + std::string(key) + "\":";
    std::vector<long> values;
    for (size_t at = json.find(needle); at != std::string_view::npos; at = json.find(needle, at)) {
        at += needle.size();
        while (at < json.size() && json[at] == ' ') {
            ++at;
        }
        long value = 0;
        auto parsed = std::from_chars(json.data() + at, json.data() + json.size(), value);
        if (parsed.ec == std::errc()) {
            values.push_back(value);
        }
    }
    return values;
}

int mostCommon(const std::vector<long> &values, int fallback) {
    std::map<long, int> counts;
    int best = fallback;
    int bestCount = 0;
    for (long value : values) {
        if (++counts[value] > bestCount) {
            bestCount = counts[value];
            best = static_cast<int>(value);
        }
    }
    return best;
}

Hyprland::Hyprland()
    : socketPath_(requireEnv("XDG_RUNTIME_DIR") + "/hypr/" +
                  requireEnv("HYPRLAND_INSTANCE_SIGNATURE") + "/.socket.sock") {}

std::string Hyprland::request(std::string_view command) const {
    sockaddr_un address{};
    address.sun_family = AF_UNIX;
    if (socketPath_.size() >= sizeof(address.sun_path)) {
        throw std::runtime_error("Hyprland socket path is too long");
    }
    socketPath_.copy(address.sun_path, socketPath_.size());

    int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0) {
        fail("socket");
    }
    std::string reply;
    try {
        if (connect(fd, reinterpret_cast<sockaddr *>(&address), sizeof(address)) < 0) {
            fail("connect to Hyprland");
        }
        if (write(fd, command.data(), command.size()) < 0) {
            fail("write to Hyprland");
        }
        char buffer[8192];
        for (;;) {
            ssize_t count = read(fd, buffer, sizeof(buffer));
            if (count < 0) {
                fail("read from Hyprland");
            }
            if (count == 0) {
                break;
            }
            reply.append(buffer, static_cast<size_t>(count));
        }
    } catch (...) {
        close(fd);
        throw;
    }
    close(fd);
    return reply;
}

int Hyprland::layoutIndex() {
    // Keyboards are switched together, but a stray device can drift (an ACPI
    // button left on another layout), so go with what most of them agree on.
    return mostCommon(jsonIntegers(request("j/devices"), "active_layout_index"), kLatinIndex);
}

void Hyprland::setLayout(int index) {
    // "all", never "current": with fcitx5 running, "current" resolves to
    // whichever device Hyprland saw last, which is often not a keyboard.
    request("/switchxkblayout all " + std::to_string(index));
}

pid_t Hyprland::focusedPid() {
    auto pids = jsonIntegers(request("j/activewindow"), "pid");
    return pids.empty() ? 0 : static_cast<pid_t>(pids.front());
}
