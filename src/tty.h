#pragma once

#include <map>
#include <set>
#include <string>
#include <sys/types.h>
#include <termios.h>
#include <unordered_map>

// Echo off with line editing still on is how getpass-style prompts read.
// Shells and full-screen programs turn both off; plain line input keeps both on.
bool isPasswordMode(tcflag_t localFlags);

// Device numbers of this user's pseudo-terminals sitting at a password prompt.
// Lists /dev/pts and opens every terminal in it.
std::set<dev_t> passwordPtys(uid_t uid);

// The same question for just these terminals (paths under /dev/pts), which is
// much cheaper when only a few of them can have changed. Path -> device number.
std::map<std::string, dev_t> passwordPtysAmong(const std::set<std::string> &paths);

struct Process {
    pid_t parent = 0;
    dev_t tty = 0;
};

// Every process with its parent and controlling terminal.
std::unordered_map<pid_t, Process> readProcesses();

// Device number from the tty_nr field of /proc/<pid>/stat.
dev_t ttyDevice(unsigned long ttyNr);

// Whether `pid` is `ancestor` or one of its descendants.
bool descendsFrom(pid_t pid, pid_t ancestor, const std::unordered_map<pid_t, Process> &processes);

// Whether any process on one of these terminals descends from the window's
// process. No list of terminal emulators is needed this way.
bool windowOwnsTty(pid_t windowPid, const std::set<dev_t> &devices,
                   const std::unordered_map<pid_t, Process> &processes);
