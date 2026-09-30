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

std::vector<std::string> jsonStrings(std::string_view json, std::string_view key) {
    std::string needle = "\"" + std::string(key) + "\":";
    std::vector<std::string> values;
    for (size_t at = json.find(needle); at != std::string_view::npos; at = json.find(needle, at)) {
        at += needle.size();
        while (at < json.size() && json[at] == ' ') {
            ++at;
        }
        if (at >= json.size() || json[at] != '"') {
            continue;
        }
        std::string value;
        for (++at; at < json.size() && json[at] != '"'; ++at) {
            if (json[at] == '\\' && at + 1 < json.size()) {
                ++at;
            }
            value += json[at];
        }
        values.push_back(std::move(value));
    }
    return values;
}

namespace {

std::vector<std::string> splitCommas(std::string_view text) {
    std::vector<std::string> parts;
    size_t start = 0;
    for (;;) {
        size_t comma = text.find(',', start);
        parts.emplace_back(text.substr(start, comma - start));
        if (comma == std::string_view::npos) {
            return parts;
        }
        start = comma + 1;
    }
}

bool isNonLatin(const std::string &layout, const std::string &variant) {
    static const std::string nonLatin =
        " af am ara bd bg by et ge gr il in iq ir kg kh kz la lk mk mm mn mv np rs ru sy th "
        "tj ua ";
    return nonLatin.find(" " + layout + " ") != std::string::npos &&
           variant.find("latin") == std::string::npos;
}

} // namespace

std::vector<bool> latinLayouts(std::string_view layouts, std::string_view variants) {
    auto names = splitCommas(layouts);
    auto kinds = splitCommas(variants);
    std::vector<bool> latin;
    for (size_t index = 0; index < names.size(); ++index) {
        latin.push_back(!names[index].empty() &&
                        !isNonLatin(names[index], index < kinds.size() ? kinds[index] : ""));
    }
    return latin;
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
    return mostCommon(jsonIntegers(request("j/devices"), "active_layout_index"), 0);
}

std::vector<bool> Hyprland::latinLayouts() {
    // Every keyboard gets the same kb_layout from the config; read the first.
    std::string devices = request("j/devices");
    auto layouts = jsonStrings(devices, "layout");
    auto variants = jsonStrings(devices, "variant");
    if (layouts.empty()) {
        return {};
    }
    return ::latinLayouts(layouts.front(), variants.empty() ? "" : variants.front());
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

HyprlandEvents::HyprlandEvents()
    : socketPath_(requireEnv("XDG_RUNTIME_DIR") + "/hypr/" +
                  requireEnv("HYPRLAND_INSTANCE_SIGNATURE") + "/.socket2.sock") {
    connect();
}

HyprlandEvents::~HyprlandEvents() {
    if (fd_ >= 0) {
        close(fd_);
    }
}

void HyprlandEvents::connect() {
    sockaddr_un address{};
    address.sun_family = AF_UNIX;
    if (socketPath_.size() >= sizeof(address.sun_path)) {
        return;
    }
    socketPath_.copy(address.sun_path, socketPath_.size());
    fd_ = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
    if (fd_ >= 0 && ::connect(fd_, reinterpret_cast<sockaddr *>(&address), sizeof(address)) < 0) {
        close(fd_);
        fd_ = -1;
    }
    partial_.clear();
}

bool HyprlandEvents::layoutChanged() {
    if (fd_ < 0) {
        connect();
        // Whatever happened while disconnected is unknown; assume a change.
        return fd_ >= 0;
    }
    bool changed = false;
    char buffer[4096];
    for (;;) {
        ssize_t count = read(fd_, buffer, sizeof(buffer));
        if (count > 0) {
            partial_.append(buffer, static_cast<size_t>(count));
            continue;
        }
        if (count == 0 || (errno != EAGAIN && errno != EINTR)) {
            // The compositor went away; try again next time.
            close(fd_);
            fd_ = -1;
        }
        break;
    }
    size_t end;
    while ((end = partial_.find('\n')) != std::string::npos) {
        changed = changed || partial_.starts_with("activelayout>>");
        partial_.erase(0, end + 1);
    }
    return changed;
}
