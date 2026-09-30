// Switch the Hyprland keyboard layout to Latin while a password is being typed.
//
// Sources that notice a password prompt call `enter`, and `leave` once it is
// gone. `watch-tty` is the source for terminal prompts (sudo, ssh, read -s).

#include "activity.h"
#include "compositor.h"
#include "state.h"
#include "tmux.h"
#include "tty.h"

#include <algorithm>
#include <chrono>
#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <exception>
#include <set>
#include <string>
#include <sys/sysmacros.h>
#include <thread>
#include <unistd.h>

namespace {

// How often to look while a prompt is up or terminals keep printing. Focus can
// move and a prompt can end without any output to announce it.
constexpr int kPollMs = 200;
// Delay before the second look after terminals wake the watcher from quiet.
constexpr int kSettleMs = 50;
const std::string kTtyHolder = "tty";

volatile std::sig_atomic_t stopRequested = 0;

void requestStop(int) { stopRequested = 1; }

struct Prompt {
    bool pending = false; // some terminal of ours is asking for a password
    bool inFocus = false; // and it belongs to the focused window
};

// Whether a prompt on one of these terminals is in front of the user in the
// focused window: the terminal belongs to the window directly, or it is the
// visible pane of a tmux session shown there.
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

// Decides whether the pending prompts belong to the focused window.
class PromptWatcher {
public:
    // `printed` are the terminals that printed since the previous look.
    Prompt look(Compositor &compositor, std::set<dev_t> devices,
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

private:
    std::set<dev_t> devices_;
    pid_t focused_ = 0;
    bool owned_ = false;
    Tmux tmux_;
    std::set<std::string> tmuxClientTerminals_;
};

int watchTty(Compositor &compositor, State &state) {
    // A previous run that died mid-prompt must not leave Latin held forever.
    state.leave(kTtyHolder, compositor);

    struct sigaction action {};
    action.sa_handler = requestStop;
    sigaction(SIGTERM, &action, nullptr);
    sigaction(SIGINT, &action, nullptr);

    TtyActivity activity;
    PromptWatcher watcher;
    bool held = false;
    // Terminals printed recently, so keep looking at a steady pace instead of
    // waking for every write.
    bool busy = true;
    // Programs print the prompt and turn echo off in either order, so the look
    // taken right on waking is followed by a second one a moment later.
    bool secondLookOwed = false;
    // A terminal's mode only needs checking after it printed something, so each
    // look covers the terminals that printed lately plus those at a prompt.
    std::set<std::string> suspects = activity.takeActive();
    std::set<std::string> printedBefore;
    while (!stopRequested) {
        bool pending = true;
        std::set<std::string> atPrompt;
        try {
            std::set<dev_t> devices;
            if (activity.available()) {
                for (const auto &[path, device] : passwordPtysAmong(suspects)) {
                    atPrompt.insert(path);
                    devices.insert(device);
                }
            } else {
                devices = passwordPtys(getuid());
            }
            Prompt prompt = watcher.look(compositor, std::move(devices), suspects);
            pending = prompt.pending;
            if (prompt.inFocus != held) {
                if (prompt.inFocus) {
                    state.enter(kTtyHolder, compositor);
                } else {
                    state.leave(kTtyHolder, compositor);
                }
                held = prompt.inFocus;
            }
        } catch (const std::exception &error) {
            // The compositor restarting drops the socket for a moment; keep
            // polling until it answers again.
            std::fprintf(stderr, "password-layout: %s\n", error.what());
            atPrompt = suspects;
        }

        if (secondLookOwed && !pending) {
            // Same terminals again.
            std::this_thread::sleep_for(std::chrono::milliseconds(kSettleMs));
            secondLookOwed = false;
            continue;
        }
        secondLookOwed = false;
        if (pending || busy || !activity.available()) {
            std::this_thread::sleep_for(std::chrono::milliseconds(kPollMs));
            busy = activity.drain() || pending;
        } else {
            // Nothing is asking for a password and the terminals are quiet:
            // sleep until one of them prints.
            busy = activity.wait(-1);
            secondLookOwed = busy;
        }
        // A terminal stays a suspect for one more look after it printed, in
        // case that look fell between the prompt and echo being turned off.
        std::set<std::string> printed = activity.takeActive();
        suspects = std::move(atPrompt);
        suspects.insert(printed.begin(), printed.end());
        suspects.insert(printedBefore.begin(), printedBefore.end());
        printedBefore = std::move(printed);
    }
    if (held) {
        state.leave(kTtyHolder, compositor);
    }
    return 0;
}

int printStatus(Compositor &compositor, State &state) {
    auto devices = passwordPtys(getuid());
    pid_t focused = compositor.focusedPid();

    std::string holders;
    for (const auto &holder : state.holders()) {
        holders += (holders.empty() ? "" : ", ") + holder;
    }
    std::string prompts;
    for (dev_t device : devices) {
        prompts += (prompts.empty() ? "pts/" : ", pts/") + std::to_string(minor(device));
    }

    std::printf("layout index:        %d\n", compositor.layoutIndex());
    std::printf("holders:             %s\n", holders.empty() ? "none" : holders.c_str());
    std::printf("password prompts:    %s\n", prompts.empty() ? "none" : prompts.c_str());
    std::printf("focused window pid:  %d\n", focused);
    Tmux tmux;
    std::set<std::string> tmuxClientTerminals;
    std::printf("prompt in focus:     %s\n",
                promptInWindow(focused, devices, readProcesses(), tmux, tmuxClientTerminals)
                    ? "yes"
                    : "no");
    return 0;
}

int usage() {
    std::fputs("usage: password-layout enter <holder>\n"
               "       password-layout leave <holder>\n"
               "       password-layout watch-tty\n"
               "       password-layout status\n",
               stderr);
    return 2;
}

} // namespace

int main(int argc, char **argv) {
    std::string command = argc > 1 ? argv[1] : "";
    bool takesHolder = command == "enter" || command == "leave";
    if (argc != (takesHolder ? 3 : 2) ||
        !(takesHolder || command == "watch-tty" || command == "status")) {
        return usage();
    }

    try {
        Hyprland compositor;
        State state;
        if (command == "enter") {
            state.enter(argv[2], compositor);
        } else if (command == "leave") {
            state.leave(argv[2], compositor);
        } else if (command == "watch-tty") {
            return watchTty(compositor, state);
        } else {
            return printStatus(compositor, state);
        }
    } catch (const std::exception &error) {
        std::fprintf(stderr, "password-layout: %s\n", error.what());
        return 1;
    }
    return 0;
}
