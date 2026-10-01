// Switch the Hyprland keyboard layout to Latin while a password is being typed.
//
// Sources that notice a password prompt call `enter`, and `leave` once it is
// gone. `watch-tty` is the source for terminal prompts (sudo, ssh, read -s).

#include "activity.h"
#include "compositor.h"
#include "doctor.h"
#include "state.h"
#include "tmux.h"
#include "tty.h"
#include "watch.h"

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

int watchTty(Compositor &compositor, State &state) {
    // A previous run that died mid-prompt must not leave Latin held forever.
    state.leave(kTtyHolder, compositor);

    struct sigaction action {};
    action.sa_handler = requestStop;
    sigaction(SIGTERM, &action, nullptr);
    sigaction(SIGINT, &action, nullptr);

    TtyActivity activity;
    PromptWatcher watcher;
    // Layout switches are followed as they happen, so that a password gets the
    // Latin layout that was last in use - macOS does the same.
    HyprlandEvents layoutEvents;
    try {
        state.noteLayout(compositor);
    } catch (const std::exception &error) {
        std::fprintf(stderr, "password-layout: %s\n", error.what());
    }
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
        if (layoutEvents.layoutChanged()) {
            try {
                state.noteLayout(compositor);
            } catch (const std::exception &error) {
                std::fprintf(stderr, "password-layout: %s\n", error.what());
            }
        }
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

        Pace pace = nextPace(secondLookOwed, pending, busy, activity.available());
        secondLookOwed = false;
        if (pace == Pace::SecondLook) {
            // Same terminals again.
            std::this_thread::sleep_for(std::chrono::milliseconds(kSettleMs));
            continue;
        }
        if (pace == Pace::Poll) {
            std::this_thread::sleep_for(std::chrono::milliseconds(kPollMs));
            busy = activity.drain() || pending;
        } else {
            busy = activity.wait(-1, layoutEvents.fd());
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
    auto last = state.lastLatin();
    std::printf("last Latin layout:   %s\n", last ? std::to_string(*last).c_str() : "none seen");
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
    std::fputs("usage: password-layout enter <holder>   hold Latin (from your own scripts)\n"
               "       password-layout leave <holder>   give it back\n"
               "       password-layout watch-tty        the terminal service\n"
               "       password-layout status           layout, holders, prompts\n"
               "       password-layout doctor           check the setup\n",
               stderr);
    return 2;
}

} // namespace

int main(int argc, char **argv) {
    std::string command = argc > 1 ? argv[1] : "";
    bool takesHolder = command == "enter" || command == "leave";
    if (argc != (takesHolder ? 3 : 2) ||
        !(takesHolder || command == "watch-tty" || command == "status" || command == "doctor")) {
        return usage();
    }
    // Before anything that needs Hyprland: finding out why it is missing is
    // part of the job.
    if (command == "doctor") {
        return printFindings(diagnose());
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
