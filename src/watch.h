#pragma once

#include "compositor.h"
#include "tmux.h"
#include "tty.h"

#include <set>
#include <string>
#include <sys/types.h>
#include <unordered_map>

// Whether a password prompt is up, and whether it is in front of the user.

struct Prompt {
    bool pending = false; // some terminal of ours is asking for a password
    bool inFocus = false; // and it belongs to the focused window
};

// Whether a prompt on one of these terminals is in front of the user in the
// focused window: the terminal belongs to the window directly, or it is the
// visible pane of a tmux session shown there.
bool promptInWindow(pid_t window, const std::set<dev_t> &devices,
                    const std::unordered_map<pid_t, Process> &processes, Tmux &tmux,
                    std::set<std::string> &tmuxClientTerminals);

// What the service loop does after a look at the terminals.
enum class Pace {
    // Look at the same terminals again in a moment: programs print the
    // prompt and turn echo off in either order.
    SecondLook,
    // Look again after the polling interval: a prompt is up (focus can move
    // and the prompt can end without any output), terminals keep printing, or
    // there is no inotify to wait on.
    Poll,
    // Nothing is asking for a password and the terminals are quiet: sleep
    // until one of them prints.
    Sleep,
};

Pace nextPace(bool secondLookOwed, bool pending, bool busy, bool canWait);

// Decides whether the pending prompts belong to the focused window.
class PromptWatcher {
public:
    // `printed` are the terminals that printed since the previous look.
    Prompt look(Compositor &compositor, std::set<dev_t> devices,
                const std::set<std::string> &printed);

private:
    std::set<dev_t> devices_;
    pid_t focused_ = 0;
    bool owned_ = false;
    Tmux tmux_;
    std::set<std::string> tmuxClientTerminals_;
};
