#include "watch.h"

#include <algorithm>

bool promptInWindow(pid_t window, const std::set<dev_t> &devices,
                    const std::unordered_map<pid_t, Process> &processes, Tmux &tmux,
                    std::set<std::string> &tmuxClientTerminals) {
    if (windowOwnsTty(window, devices, processes)) {
        return true;
    }
    bool visible = false;
    for (dev_t device : devices) {
        if (auto answer = tmux.ask(device, window, processes)) {
            tmuxClientTerminals.insert(answer->clientTerminals.begin(),
                                       answer->clientTerminals.end());
            visible = visible || answer->visibleInWindow;
        }
    }
    return visible;
}

Prompt PromptWatcher::look(Compositor &compositor, std::set<dev_t> devices,
                          const std::set<std::string> &printed) {
    // The compositor is only asked while a prompt is pending.
    if (devices.empty()) {
        devices_.clear();
        tmuxClientTerminals_.clear();
        return {};
    }
    pid_t focused = compositor.focusedPid();
    // Walking the whole process table and asking tmux are the expensive
    // parts, so they are only redone when the prompts or the focused window
    // change, or when a tmux client redrew - which is what switching panes
    // or windows inside tmux looks like from outside.
    bool tmuxRedrew = std::ranges::any_of(
        printed, [&](const auto &path) { return tmuxClientTerminals_.contains(path); });
    if (devices != devices_ || focused != focused_ || tmuxRedrew) {
        tmuxClientTerminals_.clear();
        owned_ = promptInWindow(focused, devices, readProcesses(), tmux_,
                                tmuxClientTerminals_);
        devices_ = std::move(devices);
        focused_ = focused;
    }
    return {true, owned_};
}

Pace nextPace(bool secondLookOwed, bool pending, bool busy, bool canWait) {
    if (secondLookOwed && !pending) {
        return Pace::SecondLook;
    }
    if (pending || busy || !canWait) {
        return Pace::Poll;
    }
    return Pace::Sleep;
}
