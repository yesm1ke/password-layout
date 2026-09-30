#pragma once

#include <set>
#include <string>
#include <unordered_map>

// Sleeps until a terminal does something, so the watcher need not poll, and
// remembers which terminals it was.
//
// A password prompt cannot appear silently: the program prints it. The kernel
// reports output on a pseudo-terminal, and new pseudo-terminals, through
// inotify, which is enough to know when and where a look at the terminal mode
// is due.
//
// Prompts are usually written through /dev/tty rather than the terminal's own
// device (sudo, ssh and getpass all do), and that output is reported against
// /dev/tty with no hint of which terminal it reached. It is watched too, and
// counts as output on every terminal.
class TtyActivity {
public:
    TtyActivity();
    ~TtyActivity();
    TtyActivity(const TtyActivity &) = delete;
    TtyActivity &operator=(const TtyActivity &) = delete;

    // False when inotify is unavailable; wait() then only sleeps.
    bool available() const { return fd_ >= 0; }

    // Waits up to `timeoutMs` (forever when negative) and returns whether any
    // terminal produced output or appeared. Returns early on a signal.
    bool wait(int timeoutMs);

    // Same answer for whatever has happened since the last call, without waiting.
    bool drain();

    // Paths of the terminals that printed or appeared since the last call.
    std::set<std::string> takeActive();

    // Paths of every terminal being watched.
    std::set<std::string> all() const;

private:
    void watch(const char *name);

    int fd_ = -1;
    int directoryWatch_ = -1;
    int controllingTtyWatch_ = -1;
    std::unordered_map<int, std::string> watched_; // inotify watch -> path
    std::set<std::string> active_;
};
