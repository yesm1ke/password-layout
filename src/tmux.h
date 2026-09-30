#pragma once

#include "tty.h"

#include <map>
#include <optional>
#include <set>
#include <string>
#include <string_view>
#include <sys/types.h>
#include <unordered_map>
#include <vector>

// Finds out whether a terminal is a tmux pane shown in a given window.
//
// The processes in a tmux pane descend from the tmux server, which is detached
// from any terminal, so walking up from the pane never reaches the terminal
// window it is displayed in. tmux itself knows which pane sits on which
// terminal, which pane and window are active, and which clients are attached
// to which session; a client is an ordinary process inside the terminal
// window. That is enough to connect the two.
class Tmux {
public:
    struct Answer {
        // The pane is the active one of its session and a client showing that
        // session runs in the window.
        bool visibleInWindow = false;
        // Terminals of this server's clients. Switching panes or windows in
        // tmux redraws them, which is the cue to ask again.
        std::set<std::string> clientTerminals;
    };

    // Nothing when `tty` is not a tmux pane or its server cannot be reached.
    std::optional<Answer> ask(dev_t tty, pid_t windowPid,
                              const std::unordered_map<pid_t, Process> &processes);

private:
    std::string socketOf(pid_t server);

    std::map<pid_t, std::string> sockets_; // server pid -> socket path
};

struct TmuxPane {
    std::string terminal; // "/dev/pts/N"
    std::string session;  // "$N"
    bool active = false;  // the active pane of its session's active window
};

struct TmuxClient {
    pid_t pid = 0;
    std::string session;
    std::string terminal;
};

// Parsers for the -F formats Tmux uses, kept apart so they can be tested
// without a tmux server.
inline constexpr const char *kTmuxPaneFormat =
    "#{pane_tty} #{session_id} #{window_active} #{pane_active}";
inline constexpr const char *kTmuxClientFormat = "#{client_pid} #{session_id} #{client_tty}";
std::vector<TmuxPane> parseTmuxPanes(std::string_view output);
std::vector<TmuxClient> parseTmuxClients(std::string_view output);

// Runs a command and returns what it printed, or nothing if it failed or took
// longer than `timeoutMs`. The watcher must never hang on a stuck server.
std::optional<std::string> runCommand(const std::vector<std::string> &argv, int timeoutMs = 1000);
