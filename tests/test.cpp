#include "activity.h"
#include "compositor.h"
#include "state.h"
#include "tmux.h"
#include "tty.h"

#include <chrono>
#include <csignal>
#include <cstdio>
#include <cstdlib>
#include <functional>
#include <optional>
#include <pty.h>
#include <string>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <sys/wait.h>
#include <thread>
#include <unistd.h>
#include <vector>

namespace {

int failures = 0;

#define CHECK(condition)                                                                           \
    do {                                                                                           \
        if (!(condition)) {                                                                        \
            std::printf("    FAILED %s:%d: %s\n", __FILE__, __LINE__, #condition);                 \
            ++failures;                                                                            \
        }                                                                                          \
    } while (0)

struct FakeCompositor : Compositor {
    int index;
    std::vector<int> switches;

    explicit FakeCompositor(int start) : index(start) {}
    int layoutIndex() override { return index; }
    void setLayout(int to) override {
        index = to;
        switches.push_back(to);
    }
    pid_t focusedPid() override { return 0; }
};

std::string temporaryDirectory() {
    char path[] = "/tmp/password-layout-test-XXXXXX";
    if (!mkdtemp(path)) {
        std::perror("mkdtemp");
        std::exit(1);
    }
    return path;
}

// Runs a command on a fresh pseudo-terminal and kills it when done.
class PtyChild {
public:
    explicit PtyChild(std::vector<const char *> argv) {
        argv.push_back(nullptr);
        pid_ = forkpty(&master_, nullptr, nullptr, nullptr);
        if (pid_ == 0) {
            execvp(argv[0], const_cast<char *const *>(argv.data()));
            _exit(127);
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(500));
    }
    ~PtyChild() {
        kill(pid_, SIGKILL);
        waitpid(pid_, nullptr, 0);
        close(master_);
    }
    pid_t pid() const { return pid_; }

private:
    pid_t pid_ = -1;
    int master_ = -1;
};

// Waits are bounded by time, not by a number of attempts: other terminals on
// the machine print too, and each of their events ends a wait early.
class Deadline {
public:
    explicit Deadline(int milliseconds)
        : end_(std::chrono::steady_clock::now() + std::chrono::milliseconds(milliseconds)) {}
    bool passed() const { return std::chrono::steady_clock::now() >= end_; }

private:
    std::chrono::steady_clock::time_point end_;
};

void passwordModeNeedsEchoOffAndLineInput() {
    CHECK(isPasswordMode(ICANON));
    CHECK(!isPasswordMode(ECHO | ICANON));
    CHECK(!isPasswordMode(0));
    CHECK(!isPasswordMode(ECHO));
}

void ttyDeviceDecodesProcField() {
    CHECK(ttyDevice(0x8803) == makedev(136, 3));
    // Minors past 255 spill into the high bits.
    CHECK(ttyDevice((1UL << 20) | 0x8800 | 0x2C) == makedev(136, 300));
}

void windowOwnsTtyFollowsParents() {
    dev_t tty = makedev(136, 3);
    dev_t other = makedev(136, 4);
    // terminal(100) -> shell(200) -> sudo(300); unrelated shell(400) elsewhere.
    std::unordered_map<pid_t, Process> processes{
        {100, {1, 0}}, {200, {100, tty}}, {300, {200, tty}}, {400, {1, other}}};
    CHECK(windowOwnsTty(100, {tty}, processes));
    CHECK(!windowOwnsTty(400, {tty}, processes));
    CHECK(!windowOwnsTty(0, {tty}, processes));
}

void jsonIntegersFindsEveryValue() {
    std::string devices = R"({"keyboards": [
        {"name": "a", "active_layout_index": 1, "main": false},
        {"name": "b", "active_layout_index": 0, "main": true},
        {"name": "c", "active_layout_index": 1, "main": false}]})";
    CHECK((jsonIntegers(devices, "active_layout_index") == std::vector<long>{1, 0, 1}));
    CHECK(mostCommon(jsonIntegers(devices, "active_layout_index"), 0) == 1);
    CHECK(jsonIntegers("{}", "pid").empty());
    CHECK(mostCommon({}, 7) == 7);
}

void jsonIntegersIgnoresKeysInsideStrings() {
    // A window title is user-controlled text; its quotes arrive escaped.
    std::string window = R"({"title": "fake \"pid\": 999 here", "pid": 3309, "xwayland": false})";
    CHECK((jsonIntegers(window, "pid") == std::vector<long>{3309}));
}

void stateSwitchesToLatinAndBack() {
    State state(temporaryDirectory());
    FakeCompositor compositor(1);
    state.enter("tty", compositor);
    CHECK(compositor.index == 0);
    CHECK((state.holders() == std::vector<std::string>{"tty"}));
    state.leave("tty", compositor);
    CHECK((compositor.switches == std::vector<int>{0, 1}));
    CHECK(state.holders().empty());
}

void stateLeavesLatinAlone() {
    State state(temporaryDirectory());
    FakeCompositor compositor(0);
    state.enter("tty", compositor);
    state.leave("tty", compositor);
    CHECK(compositor.switches.empty());
}

void stateRestoresOnlyAfterLastHolder() {
    State state(temporaryDirectory());
    FakeCompositor compositor(1);
    state.enter("tty", compositor);
    state.enter("im", compositor);
    state.leave("tty", compositor);
    CHECK(compositor.index == 0);
    state.leave("im", compositor);
    CHECK((compositor.switches == std::vector<int>{0, 1}));
}

void stateIgnoresLeaveWithoutEnter() {
    State state(temporaryDirectory());
    FakeCompositor compositor(1);
    state.leave("tty", compositor);
    CHECK(compositor.switches.empty());
}

void silentReadIsAPromptOwnedByUs() {
    PtyChild child({"bash", "-c", "read -s -p pw: x"});
    auto devices = passwordPtys(getuid());
    auto processes = readProcesses();
    CHECK(devices.contains(processes[child.pid()].tty));
    CHECK(windowOwnsTty(getpid(), devices, processes));
}

void plainInputIsNotAPrompt() {
    PtyChild child({"cat"});
    auto processes = readProcesses();
    CHECK(processes.contains(child.pid()));
    CHECK(!passwordPtys(getuid()).contains(processes[child.pid()].tty));
}

// Other terminals on the machine print during a test too, so these tests ask
// about the child's own terminal rather than counting what was reported.
std::string terminalOf(const PtyChild &child) {
    auto processes = readProcesses();
    return "/dev/pts/" + std::to_string(minor(processes[child.pid()].tty));
}

void terminalOutputNamesTheTerminal() {
    TtyActivity activity;
    CHECK(activity.available());
    activity.takeActive();
    // Started after the watcher exists: the new terminal is reported first, and
    // once that is forgotten, what is left to report is its output.
    PtyChild child({"bash", "-c", "sleep 1; echo hello; sleep 5"});
    std::string path = terminalOf(child);
    activity.drain();
    CHECK(activity.takeActive().contains(path));
    CHECK(activity.all().contains(path));

    bool printed = false;
    for (Deadline deadline(5000); !printed && !deadline.passed();) {
        activity.wait(200);
        printed = activity.takeActive().contains(path);
    }
    CHECK(printed);
}

void outputThroughDevTtyMakesEveryTerminalSuspect() {
    PtyChild child({"bash", "-c", "sleep 1; echo prompt > /dev/tty; sleep 5"});
    TtyActivity activity;
    activity.takeActive();
    std::string path = terminalOf(child);
    bool reported = false;
    for (Deadline deadline(5000); !reported && !deadline.passed();) {
        activity.wait(200);
        reported = activity.takeActive().contains(path);
    }
    CHECK(reported);
}

void promptIsFoundAmongGivenTerminals() {
    PtyChild prompting({"bash", "-c", "read -s -p pw: x"});
    PtyChild plain({"cat"});
    std::string path = terminalOf(prompting);
    auto prompts = passwordPtysAmong({path, terminalOf(plain)});
    CHECK(prompts.size() == 1);
    CHECK(prompts.contains(path));
    CHECK(prompts[path] == readProcesses()[prompting.pid()].tty);
    CHECK(passwordPtysAmong({}).empty());
}

void tmuxOutputIsParsed() {
    auto panes = parseTmuxPanes("/dev/pts/7 $0 1 1\n/dev/pts/8 $0 1 0\n/dev/pts/9 $1 0 1\n");
    CHECK(panes.size() == 3);
    CHECK(panes.size() == 3 && panes[0].terminal == "/dev/pts/7" && panes[0].session == "$0" &&
          panes[0].active);
    CHECK(panes.size() == 3 && !panes[1].active); // pane in the active window, not active itself
    CHECK(panes.size() == 3 && !panes[2].active); // active pane of a window in the background

    auto clients = parseTmuxClients("4242 $0 /dev/pts/3\n");
    CHECK(clients.size() == 1);
    CHECK(clients.size() == 1 && clients[0].pid == 4242 && clients[0].session == "$0" &&
          clients[0].terminal == "/dev/pts/3");
    CHECK(parseTmuxPanes("").empty() && parseTmuxClients("").empty());
}

void commandsAreRunAndTimedOut() {
    CHECK(runCommand({"echo", "hello"}) == std::optional<std::string>("hello\n"));
    CHECK(!runCommand({"false"}));
    CHECK(!runCommand({"password-layout-no-such-command"}));
    auto started = std::chrono::steady_clock::now();
    CHECK(!runCommand({"sleep", "5"}, 200));
    CHECK(std::chrono::steady_clock::now() - started < std::chrono::seconds(2));
}

// A private tmux server for one test, gone afterwards.
class TmuxServer {
public:
    static constexpr const char *kName = "password-layout-test";
    TmuxServer() { tmux({"kill-server"}); }
    ~TmuxServer() { tmux({"kill-server"}); }
    static std::optional<std::string> tmux(std::vector<std::string> args) {
        args.insert(args.begin(), {"tmux", "-L", kName});
        return runCommand(args, 3000);
    }
};

void tmuxPaneIsSeenOnlyWhileVisible() {
    if (!runCommand({"tmux", "-V"})) {
        std::printf("    skipped: tmux is not installed\n");
        return;
    }
    TmuxServer server;
    // Window numbering depends on the user's base-index, so windows are named
    // by the ids tmux prints for them.
    auto first = TmuxServer::tmux({"new-session", "-d", "-P", "-F", "#{pane_tty}", "-s", "t",
                                   "-x", "80", "-y", "24", "bash -c 'read -s -p pw: x; sleep 30'"});
    auto second = TmuxServer::tmux({"new-window", "-d", "-P", "-F", "#{window_id}", "-t", "t:",
                                    "sleep 30"});
    CHECK(first && second);
    if (!first || !second) {
        return;
    }
    // The client runs on a terminal this test process started, so the test
    // plays the part of the terminal window.
    PtyChild client({"tmux", "-L", TmuxServer::kName, "attach", "-t", "t"});

    struct stat info;
    std::string path = first->substr(0, first->find('\n'));
    CHECK(stat(path.c_str(), &info) == 0);
    dev_t pane = info.st_rdev;

    Tmux tmux;
    auto shown = tmux.ask(pane, getpid(), readProcesses());
    CHECK(shown && shown->visibleInWindow);
    CHECK(shown && shown->clientTerminals.size() == 1);
    CHECK(passwordPtysAmong({path}).contains(path)); // and the pane is at the prompt

    // Another window, the one the client is not running in.
    auto elsewhere = tmux.ask(pane, client.pid() + 100000, readProcesses());
    CHECK(elsewhere && !elsewhere->visibleInWindow);

    // Switch the session to its second window: the pane is out of sight.
    CHECK(TmuxServer::tmux({"select-window", "-t", second->substr(0, second->find('\n'))}));
    auto hidden = tmux.ask(pane, getpid(), readProcesses());
    CHECK(hidden && !hidden->visibleInWindow);

    // A terminal that is not a tmux pane at all.
    PtyChild plain({"cat"});
    CHECK(!tmux.ask(readProcesses()[plain.pid()].tty, getpid(), readProcesses()));
}

} // namespace

int main() {
    const std::pair<const char *, std::function<void()>> tests[] = {
        {"password mode needs echo off and line input", passwordModeNeedsEchoOffAndLineInput},
        {"tty device decodes the /proc field", ttyDeviceDecodesProcField},
        {"window owns tty follows parents", windowOwnsTtyFollowsParents},
        {"json integers finds every value", jsonIntegersFindsEveryValue},
        {"json integers ignores keys inside strings", jsonIntegersIgnoresKeysInsideStrings},
        {"state switches to Latin and back", stateSwitchesToLatinAndBack},
        {"state leaves Latin alone", stateLeavesLatinAlone},
        {"state restores only after the last holder", stateRestoresOnlyAfterLastHolder},
        {"state ignores leave without enter", stateIgnoresLeaveWithoutEnter},
        {"silent read is a prompt owned by us", silentReadIsAPromptOwnedByUs},
        {"plain input is not a prompt", plainInputIsNotAPrompt},
        {"terminal output names the terminal", terminalOutputNamesTheTerminal},
        {"output through /dev/tty makes every terminal suspect", outputThroughDevTtyMakesEveryTerminalSuspect},
        {"prompt is found among given terminals", promptIsFoundAmongGivenTerminals},
        {"tmux output is parsed", tmuxOutputIsParsed},
        {"commands are run and timed out", commandsAreRunAndTimedOut},
        {"tmux pane is seen only while visible", tmuxPaneIsSeenOnlyWhileVisible},
    };
    for (const auto &[name, test] : tests) {
        int before = failures;
        test();
        std::printf("%s  %s\n", failures == before ? "ok  " : "FAIL", name);
    }
    std::printf("\n%zu tests, %d failed checks\n", std::size(tests), failures);
    return failures ? 1 : 0;
}
