#include "tty.h"

#include <cctype>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <dirent.h>
#include <fcntl.h>
#include <string>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <unistd.h>

namespace {

bool isNumber(const char *name) {
    if (!*name) {
        return false;
    }
    for (; *name; ++name) {
        if (!std::isdigit(static_cast<unsigned char>(*name))) {
            return false;
        }
    }
    return true;
}

} // namespace

bool isPasswordMode(tcflag_t localFlags) {
    return !(localFlags & ECHO) && (localFlags & ICANON);
}

namespace {

// Whether the terminal at `path` is at a password prompt; fills in its device
// number when it is.
bool atPasswordPrompt(const char *path, dev_t &device) {
    // Opened and closed on every look: a descriptor kept on the slave would
    // stop the terminal from seeing its shell exit.
    int fd = open(path, O_RDONLY | O_NOCTTY | O_NONBLOCK | O_CLOEXEC);
    if (fd < 0) {
        return false;
    }
    termios settings;
    struct stat info;
    bool prompt = tcgetattr(fd, &settings) == 0 && isPasswordMode(settings.c_lflag) &&
                  fstat(fd, &info) == 0;
    if (prompt) {
        device = info.st_rdev;
    }
    close(fd);
    return prompt;
}

} // namespace

std::set<dev_t> passwordPtys(uid_t uid) {
    std::set<dev_t> found;
    DIR *directory = opendir("/dev/pts");
    if (!directory) {
        return found;
    }
    while (dirent *entry = readdir(directory)) {
        if (!isNumber(entry->d_name)) {
            continue;
        }
        std::string path = std::string("/dev/pts/") + entry->d_name;
        struct stat info;
        dev_t device;
        if (stat(path.c_str(), &info) == 0 && info.st_uid == uid &&
            atPasswordPrompt(path.c_str(), device)) {
            found.insert(device);
        }
    }
    closedir(directory);
    return found;
}

std::map<std::string, dev_t> passwordPtysAmong(const std::set<std::string> &paths) {
    std::map<std::string, dev_t> found;
    for (const auto &path : paths) {
        dev_t device;
        if (atPasswordPrompt(path.c_str(), device)) {
            found.emplace(path, device);
        }
    }
    return found;
}

dev_t ttyDevice(unsigned long ttyNr) {
    unsigned int major = (ttyNr >> 8) & 0xFFF;
    unsigned int minor = (ttyNr & 0xFF) | ((ttyNr >> 12) & 0xFFF00);
    return makedev(major, minor);
}

std::unordered_map<pid_t, Process> readProcesses() {
    std::unordered_map<pid_t, Process> processes;
    DIR *directory = opendir("/proc");
    if (!directory) {
        return processes;
    }
    while (dirent *entry = readdir(directory)) {
        if (!isNumber(entry->d_name)) {
            continue;
        }
        std::string path = std::string("/proc/") + entry->d_name + "/stat";
        int fd = open(path.c_str(), O_RDONLY | O_CLOEXEC);
        if (fd < 0) {
            continue;
        }
        char buffer[1024];
        ssize_t count = read(fd, buffer, sizeof(buffer) - 1);
        close(fd);
        if (count <= 0) {
            continue;
        }
        buffer[count] = '\0';
        // The command name sits in parentheses and may itself contain spaces
        // and parentheses, so the fields are counted from the last ")".
        const char *fields = std::strrchr(buffer, ')');
        if (!fields) {
            continue;
        }
        char state;
        int parent, group, session;
        unsigned long ttyNr;
        if (std::sscanf(fields + 1, " %c %d %d %d %lu", &state, &parent, &group, &session,
                        &ttyNr) != 5) {
            continue;
        }
        processes[std::atoi(entry->d_name)] = Process{parent, ttyDevice(ttyNr)};
    }
    closedir(directory);
    return processes;
}

bool descendsFrom(pid_t pid, pid_t ancestor, const std::unordered_map<pid_t, Process> &processes) {
    if (ancestor <= 0) {
        return false;
    }
    // Bounded, in case a parent chain read mid-change ever loops.
    for (size_t steps = 0; pid > 1 && steps <= processes.size(); ++steps) {
        if (pid == ancestor) {
            return true;
        }
        auto parent = processes.find(pid);
        pid = parent == processes.end() ? 0 : parent->second.parent;
    }
    return false;
}

bool windowOwnsTty(pid_t windowPid, const std::set<dev_t> &devices,
                   const std::unordered_map<pid_t, Process> &processes) {
    for (const auto &[pid, process] : processes) {
        if (devices.contains(process.tty) && descendsFrom(pid, windowPid, processes)) {
            return true;
        }
    }
    return false;
}
