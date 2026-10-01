#include "doctor.h"

#include "compositor.h"
#include "tmux.h"

#include <cstdio>
#include <cstdlib>
#include <dirent.h>
#include <filesystem>
#include <fstream>
#include <map>
#include <sstream>
#include <sys/stat.h>
#include <unistd.h>

namespace {

constexpr const char *kUnit = "password-layout-tty.service";
constexpr const char *kAddon = "libpasswordlayout.so";

std::string trim(std::string text) {
    while (!text.empty() && (text.back() == '\n' || text.back() == ' ')) {
        text.pop_back();
    }
    return text;
}

std::string readFile(const std::string &path) {
    std::ifstream file(path);
    std::stringstream text;
    text << file.rdbuf();
    return text.str();
}

std::vector<std::string> split(std::string_view text, char separator) {
    std::vector<std::string> parts;
    size_t start = 0;
    for (;;) {
        size_t end = text.find(separator, start);
        parts.emplace_back(text.substr(start, end - start));
        if (end == std::string_view::npos) {
            return parts;
        }
        start = end + 1;
    }
}

Finding checkHyprland() {
    try {
        Hyprland hyprland;
        auto latin = hyprland.latinLayouts();
        std::string list = hyprland.layoutList();
        if (latin.empty()) {
            return {Finding::Level::Fail, "Hyprland: the keyboard layouts could not be read", ""};
        }
        std::string latinNames;
        auto names = split(list, ',');
        for (size_t index = 0; index < latin.size() && index < names.size(); ++index) {
            if (latin[index]) {
                latinNames += (latinNames.empty() ? "" : ", ") + names[index];
            }
        }
        if (latinNames.empty()) {
            return {Finding::Level::Fail,
                    "Hyprland: no Latin layout among \"" + list + "\", so there is nothing to "
                    "switch to",
                    "add one, such as us, to kb_layout in Hyprland's input settings"};
        }
        return {Finding::Level::Ok, "Hyprland: layouts " + list + "; Latin: " + latinNames, ""};
    } catch (const std::exception &error) {
        return {Finding::Level::Fail, std::string("Hyprland is not reachable: ") + error.what(),
                "run this inside your Hyprland session"};
    }
}

Finding checkService() {
    auto shown = runCommand(
        {"systemctl", "--user", "show", kUnit, "-p", "LoadState", "-p", "ActiveState", "-p",
         "NRestarts", "-p", "UnitFileState", "-p", "MainPID"});
    if (!shown) {
        return {Finding::Level::Fail, "terminal service: systemctl --user does not answer",
                "the service needs a systemd user session"};
    }
    std::map<std::string, std::string> properties;
    for (const auto &line : split(*shown, '\n')) {
        if (auto equals = line.find('='); equals != std::string::npos) {
            properties[line.substr(0, equals)] = line.substr(equals + 1);
        }
    }
    if (properties["LoadState"] != "loaded") {
        return {Finding::Level::Fail, "terminal service: not installed",
                "install the package, or run make install-user from a checkout"};
    }
    std::string state = properties["ActiveState"];
    int restarts = std::atoi(properties["NRestarts"].c_str());
    bool enabled = properties["UnitFileState"] == "enabled";
    if (state == "active" && restarts == 0) {
        std::error_code error;
        auto exe = std::filesystem::read_symlink("/proc/" + properties["MainPID"] + "/exe", error);
        if (!error && exeReplaced(exe.string())) {
            return {Finding::Level::Warn,
                    "terminal service: running the binary of a package that has been updated "
                    "since",
                    "systemctl --user restart password-layout-tty"};
        }
        if (!enabled) {
            return {Finding::Level::Warn,
                    "terminal service: running, but not enabled: it will not start at the next "
                    "login",
                    "systemctl --user enable password-layout-tty"};
        }
        return {Finding::Level::Ok, "terminal service: running", ""};
    }
    if (restarts > 0) {
        std::string hint = "see journalctl --user -u password-layout-tty";
        if (trim(readFile("/proc/sys/kernel/unprivileged_userns_clone")) == "0") {
            hint = "this kernel has unprivileged user namespaces turned off, which part of the "
                   "sandbox needs; turn that part off and restart the service:\n"
                   "mkdir -p ~/.config/systemd/user/password-layout-tty.service.d && touch "
                   "~/.config/systemd/user/password-layout-tty.service.d/"
                   "sandbox-namespaces.conf && systemctl --user restart password-layout-tty";
        }
        return {state == "active" ? Finding::Level::Warn : Finding::Level::Fail,
                "terminal service: " + state + ", restarted " + std::to_string(restarts) +
                    " times",
                hint};
    }
    return {Finding::Level::Fail,
            "terminal service: " + state + (enabled ? "" : ", not enabled"),
            "systemctl --user enable --now password-layout-tty"};
}

// fcitx5 processes of this user.
std::vector<pid_t> fcitxProcesses() {
    std::vector<pid_t> pids;
    DIR *directory = opendir("/proc");
    if (!directory) {
        return pids;
    }
    while (dirent *entry = readdir(directory)) {
        std::string base = std::string("/proc/") + entry->d_name;
        struct stat info;
        if (entry->d_name[0] < '1' || entry->d_name[0] > '9' || stat(base.c_str(), &info) != 0 ||
            info.st_uid != getuid()) {
            continue;
        }
        if (trim(readFile(base + "/comm")) == "fcitx5") {
            pids.push_back(std::atoi(entry->d_name));
        }
    }
    closedir(directory);
    return pids;
}

Finding checkFcitx() {
    auto pids = fcitxProcesses();
    if (pids.empty()) {
        return {Finding::Level::Warn,
                "fcitx5 is not running: password fields in graphical applications are not "
                "covered, terminals are",
                "start fcitx5 as the input method (Omarchy does this by default)"};
    }
    for (pid_t pid : pids) {
        switch (mapsLibrary(readFile("/proc/" + std::to_string(pid) + "/maps"), kAddon)) {
        case Mapped::Yes:
            return {Finding::Level::Ok, "fcitx5: running, addon loaded", ""};
        case Mapped::Replaced:
            return {Finding::Level::Warn,
                    "fcitx5 runs the addon of a package that has been updated since",
                    "restart fcitx5 to load the new one: fcitx5 -rd (on Omarchy: systemctl "
                    "--user restart omarchy-fcitx5)"};
        case Mapped::No:
            break;
        }
    }
    // fcitx5 skips, without a word in its log, an addon that asks for a newer
    // core than the running one.
    auto running = runCommand({"fcitx5", "--version"});
    for (const char *conf : {"/usr/share/fcitx5/addon/passwordlayout.conf",
                             "/usr/local/share/fcitx5/addon/passwordlayout.conf"}) {
        auto required = addonRequiresCore(readFile(conf));
        if (running && required && versionLess(trim(*running), *required)) {
            return {Finding::Level::Warn,
                    "fcitx5 " + trim(*running) + " is running without the addon, which needs " +
                        *required + " or newer",
                    "update the system (sudo pacman -Syu), or install a package built for this "
                    "fcitx5"};
        }
    }
    return {Finding::Level::Warn, "fcitx5 is running without the addon",
            "restart fcitx5 so it loads it: fcitx5 -rd (on Omarchy: systemctl --user restart "
            "omarchy-fcitx5)"};
}

std::optional<Finding> checkFcitxQt() {
    auto installed = runCommand({"pacman", "-Q", "fcitx5-qt"});
    if (!installed) {
        return std::nullopt;
    }
    auto fields = split(trim(*installed), ' ');
    if (fields.size() < 2 || !versionLess(fields[1], "5.1.16")) {
        return std::nullopt;
    }
    return Finding{Finding::Level::Warn,
                   "fcitx5-qt " + fields[1] +
                       ": Qt windows that open with a password field already focused "
                       "(KeePassXC, the polkit prompt, possibly the lock screen) are not "
                       "recognised until focus moves",
                   "fixed upstream in fcitx5-qt 5.1.16; update once your distribution ships it"};
}

std::optional<Finding> checkSudo() {
    auto output = runCommand({"sudo", "-V"});
    auto version = output ? sudoVersion(*output) : std::nullopt;
    if (!version || versionLess(*version, "1.9.14")) {
        return std::nullopt;
    }
    return Finding{Finding::Level::Info,
                   "sudo " + *version +
                       " runs commands in a terminal of its own: sudo's password prompt is "
                       "recognised, prompts of the programs it runs are not",
                   ""};
}

} // namespace

bool versionLess(std::string_view a, std::string_view b) {
    auto numbers = [](std::string_view text) {
        std::vector<long> parts;
        long value = -1;
        for (char c : text) {
            if (c >= '0' && c <= '9') {
                value = (value < 0 ? 0 : value * 10) + (c - '0');
            } else if (c == '.' && value >= 0) {
                parts.push_back(value);
                value = -1;
            } else {
                break;
            }
        }
        if (value >= 0) {
            parts.push_back(value);
        }
        return parts;
    };
    return numbers(a) < numbers(b);
}

std::optional<std::string> sudoVersion(std::string_view output) {
    constexpr std::string_view prefix = "Sudo version ";
    if (!output.starts_with(prefix)) {
        return std::nullopt;
    }
    auto line = output.substr(prefix.size());
    return std::string(line.substr(0, line.find('\n')));
}

std::optional<std::string> addonRequiresCore(std::string_view conf) {
    constexpr std::string_view key = "=core:";
    for (const auto &line : split(conf, '\n')) {
        if (auto at = line.find(key); at != std::string::npos) {
            return trim(line.substr(at + key.size()));
        }
    }
    return std::nullopt;
}

Mapped mapsLibrary(std::string_view maps, std::string_view fileName) {
    std::string needle = "/" + std::string(fileName);
    Mapped found = Mapped::No;
    for (const auto &line : split(maps, '\n')) {
        if (line.ends_with(needle)) {
            return Mapped::Yes;
        }
        if (line.ends_with(needle + " (deleted)")) {
            found = Mapped::Replaced;
        }
    }
    return found;
}

bool exeReplaced(std::string_view link) { return link.ends_with(" (deleted)"); }

std::vector<Finding> diagnose() {
    std::vector<Finding> findings{checkHyprland(), checkService(), checkFcitx()};
    for (auto optional : {checkFcitxQt(), checkSudo()}) {
        if (optional) {
            findings.push_back(*optional);
        }
    }
    return findings;
}

int printFindings(const std::vector<Finding> &findings) {
    bool failed = false;
    for (const auto &finding : findings) {
        const char *label = "ok  ";
        switch (finding.level) {
        case Finding::Level::Ok: break;
        case Finding::Level::Info: label = "note"; break;
        case Finding::Level::Warn: label = "warn"; break;
        case Finding::Level::Fail: label = "FAIL"; failed = true; break;
        }
        std::printf("%s  %s\n", label, finding.what.c_str());
        for (const auto &line : split(finding.hint, '\n')) {
            if (!line.empty()) {
                std::printf("      -> %s\n", line.c_str());
            }
        }
    }
    return failed ? 1 : 0;
}
