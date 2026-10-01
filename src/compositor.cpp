#include "compositor.h"

#include <cerrno>
#include <charconv>
#include <cstdlib>
#include <map>
#include <stdexcept>
#include <sys/socket.h>
#include <sys/time.h>
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

// Where the value of each `"key":` starts, in order of appearance.
std::vector<size_t> valuesOf(std::string_view json, std::string_view key) {
    std::string needle = "\"" + std::string(key) + "\":";
    std::vector<size_t> starts;
    for (size_t at = json.find(needle); at != std::string_view::npos; at = json.find(needle, at)) {
        at += needle.size();
        while (at < json.size() && json[at] == ' ') {
            ++at;
        }
        starts.push_back(at);
    }
    return starts;
}

// A connected Unix stream socket, or -1 with errno set.
int connectTo(const std::string &path, int flags) {
    sockaddr_un address{};
    address.sun_family = AF_UNIX;
    if (path.size() >= sizeof(address.sun_path)) {
        errno = ENAMETOOLONG;
        return -1;
    }
    path.copy(address.sun_path, path.size());
    int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC | flags, 0);
    if (fd >= 0 && ::connect(fd, reinterpret_cast<sockaddr *>(&address), sizeof(address)) < 0) {
        int saved = errno;
        close(fd);
        errno = saved;
        return -1;
    }
    return fd;
}

} // namespace

std::vector<long> jsonIntegers(std::string_view json, std::string_view key) {
    std::vector<long> values;
    for (size_t at : valuesOf(json, key)) {
        long value = 0;
        auto parsed = std::from_chars(json.data() + at, json.data() + json.size(), value);
        if (parsed.ec == std::errc()) {
            values.push_back(value);
        }
    }
    return values;
}

std::vector<std::string> jsonStrings(std::string_view json, std::string_view key) {
    std::vector<std::string> values;
    for (size_t at : valuesOf(json, key)) {
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

// Whether a layout types Latin letters, measured rather than guessed: every
// layout and variant of xkeyboard-config 2.48 was compiled with libxkbcommon
// and counts as Latin if its base level types at least 20 of the 26 letters
// a-z (tools/xkb-latin.c; the result is tests/xkb-latin.tsv, which the tests
// check this against). Esperanto or Azerbaijani, a few letters short, stay
// Latin; a script with a stray Latin letter does not. Rerun the tool when
// xkeyboard-config gains layouts.
bool isLatin(const std::string &layout, const std::string &variant) {
    // Layouts whose base form types no Latin.
    static const std::string nonLatin =
        " af am ara bd bg brai bt by eg et ge gn gr il in iq ir kg kh kz la lk ma mk mm mn "
        "mv my np pk rs ru sy th tj tz ua uz ";
    // Variants that differ from their layout, beyond those named *latin*.
    static const std::string latinVariants =
        " in(eng) in(mara) iq(ku) iq(ku_alt) iq(ku_f) ir(ku) ir(ku_alt) ir(ku_f) lk(us) "
        "ma(french) ma(rif) mm(mara) ru(ruchey_en) sy(ku) sy(ku_alt) sy(ku_f) ua(crh) "
        "ua(crh_alt) ua(crh_f) ";
    static const std::string nonLatinVariants =
        " al(veqilharxhi) az(cyrillic) br(rus) ca(ike) cn(mon_manchu_galik) cn(mon_todo_galik) "
        "cn(mon_trad) cn(mon_trad_galik) cn(mon_trad_manchu) cn(mon_trad_todo) "
        "cn(mon_trad_xibe) cn(tib) cn(tib_asciinum) cn(ug) cz(rus) cz(ucw) de(ru) dz(ar) "
        "dz(ber) fr(geo) id(javanese) id(melayu-phonetic) id(melayu-phoneticx) "
        "id(pegon-phonetic) ie(ogam) it(geo) jp(kana) jp(mac) lv(modern-cyr) me(cyrillic) "
        "me(cyrillicalternatequotes) me(cyrillicyz) ph(capewell-dvorak-bay) "
        "ph(capewell-qwerf2k6-bay) ph(colemak-bay) ph(dvorak-bay) ph(qwerty-bay) "
        "pl(ru_phonetic_dvorak) se(rus) se(swl) us(chr) us(rus) ";
    auto listed = [](const std::string &list, const std::string &name) {
        return list.find(" " + name + " ") != std::string::npos;
    };
    if (!variant.empty()) {
        std::string named = layout + "(" + variant + ")";
        if (listed(nonLatinVariants, named)) {
            return false;
        }
        if (listed(latinVariants, named) || variant.find("latin") != std::string::npos) {
            return true;
        }
    }
    return !listed(nonLatin, layout);
}

} // namespace

std::vector<bool> latinLayouts(std::string_view layouts, std::string_view variants) {
    auto names = splitCommas(layouts);
    auto kinds = splitCommas(variants);
    std::vector<bool> latin;
    for (size_t index = 0; index < names.size(); ++index) {
        latin.push_back(!names[index].empty() &&
                        isLatin(names[index], index < kinds.size() ? kinds[index] : ""));
    }
    return latin;
}

std::pair<std::string, std::string> keyboardLayouts(std::string_view devices) {
    // Like layoutIndex(): what most keyboards have, not the first one listed
    // (which may be a virtual or ACPI device), and not the one Hyprland marks
    // "main" (here an ACPI sleep button).
    auto layouts = jsonStrings(devices, "layout");
    auto variants = jsonStrings(devices, "variant");
    if (layouts.empty()) {
        return {};
    }
    std::map<std::string, int> counts;
    size_t best = 0;
    for (size_t index = 0; index < layouts.size(); ++index) {
        if (++counts[layouts[index]] > counts[layouts[best]]) {
            best = index;
        }
    }
    // Every keyboard entry carries both keys, so the lists line up.
    bool aligned = variants.size() == layouts.size();
    return {layouts[best], aligned ? variants[best] : std::string()};
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
    int fd = connectTo(socketPath_, 0);
    if (fd < 0) {
        fail("connect to Hyprland");
    }
    std::string reply;
    try {
        // A hung compositor must not hang the service, nor fcitx5's exit,
        // which waits for the addon's worker.
        timeval timeout{1, 0};
        setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout));
        setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof(timeout));
        // MSG_NOSIGNAL: a compositor that closes the socket first must not
        // kill the service with SIGPIPE.
        if (send(fd, command.data(), command.size(), MSG_NOSIGNAL) < 0) {
            fail("write to Hyprland");
        }
        char buffer[8192];
        for (;;) {
            ssize_t count = read(fd, buffer, sizeof(buffer));
            if (count < 0 && (errno == EAGAIN || errno == EWOULDBLOCK)) {
                throw std::runtime_error("Hyprland did not answer within a second");
            }
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
    auto [layouts, variants] = keyboardLayouts(request("j/devices"));
    return layouts.empty() ? std::vector<bool>{} : ::latinLayouts(layouts, variants);
}

std::string Hyprland::layoutList() { return keyboardLayouts(request("j/devices")).first; }

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
    fd_ = connectTo(socketPath_, SOCK_NONBLOCK);
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
