#include "tmux.h"

#include <cerrno>
#include <chrono>
#include <csignal>
#include <cstdlib>
#include <dirent.h>
#include <fcntl.h>
#include <fstream>
#include <poll.h>
#include <sstream>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <sys/wait.h>
#include <unistd.h>

namespace {

std::string terminalPath(dev_t tty) { return "/dev/pts/" + std::to_string(minor(tty)); }

std::string processName(pid_t pid) {
    std::ifstream file("/proc/" + std::to_string(pid) + "/comm");
    std::string name;
    std::getline(file, name);
    return name;
}

// The tmux server behind a pane: the parent of a process on its terminal.
std::optional<pid_t> serverOf(dev_t tty, const std::unordered_map<pid_t, Process> &processes) {
    for (const auto &[pid, process] : processes) {
        if (process.tty == tty && process.parent > 1 &&
            processName(process.parent).starts_with("tmux")) {
            return process.parent;
        }
    }
    return std::nullopt;
}

std::vector<std::string> socketsInDirectory() {
    const char *base = std::getenv("TMUX_TMPDIR");
    std::string directory = std::string(base && *base ? base : "/tmp") + "/tmux-" +
                            std::to_string(getuid());
    std::vector<std::string> sockets;
    if (DIR *listing = opendir(directory.c_str())) {
        while (dirent *entry = readdir(listing)) {
            std::string path = directory + "/" + entry->d_name;
            struct stat info;
            if (stat(path.c_str(), &info) == 0 && S_ISSOCK(info.st_mode)) {
                sockets.push_back(path);
            }
        }
        closedir(listing);
    }
    return sockets;
}

} // namespace

std::optional<std::string> runCommand(const std::vector<std::string> &argv, int timeoutMs) {
    int output[2];
    if (pipe2(output, O_CLOEXEC) < 0) {
        return std::nullopt;
    }
    pid_t child = fork();
    if (child < 0) {
        close(output[0]);
        close(output[1]);
        return std::nullopt;
    }
    if (child == 0) {
        int null = open("/dev/null", O_RDWR);
        dup2(null, STDIN_FILENO);
        dup2(output[1], STDOUT_FILENO);
        dup2(null, STDERR_FILENO);
        std::vector<char *> args;
        for (const auto &arg : argv) {
            args.push_back(const_cast<char *>(arg.c_str()));
        }
        args.push_back(nullptr);
        execvp(args[0], args.data());
        _exit(127);
    }
    close(output[1]);

    std::string text;
    auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(timeoutMs);
    bool timedOut = false;
    for (;;) {
        auto left = std::chrono::duration_cast<std::chrono::milliseconds>(
            deadline - std::chrono::steady_clock::now());
        if (left.count() <= 0) {
            timedOut = true;
            break;
        }
        pollfd descriptor{output[0], POLLIN, 0};
        int ready = poll(&descriptor, 1, static_cast<int>(left.count()));
        if (ready < 0 && errno == EINTR) {
            continue;
        }
        if (ready <= 0) {
            timedOut = ready == 0;
            break;
        }
        char buffer[4096];
        ssize_t count = read(output[0], buffer, sizeof(buffer));
        if (count <= 0) {
            break;
        }
        text.append(buffer, static_cast<size_t>(count));
    }
    close(output[0]);
    if (timedOut) {
        kill(child, SIGKILL);
    }
    int status = 0;
    while (waitpid(child, &status, 0) < 0 && errno == EINTR) {
    }
    if (timedOut || !WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        return std::nullopt;
    }
    return text;
}

std::vector<TmuxPane> parseTmuxPanes(std::string_view output) {
    std::vector<TmuxPane> panes;
    std::istringstream lines{std::string(output)};
    std::string terminal, session;
    int windowActive = 0, paneActive = 0;
    while (lines >> terminal >> session >> windowActive >> paneActive) {
        panes.push_back({terminal, session, windowActive == 1 && paneActive == 1});
    }
    return panes;
}

std::vector<TmuxClient> parseTmuxClients(std::string_view output) {
    std::vector<TmuxClient> clients;
    std::istringstream lines{std::string(output)};
    pid_t pid = 0;
    std::string session, terminal;
    while (lines >> pid >> session >> terminal) {
        clients.push_back({pid, session, terminal});
    }
    return clients;
}

namespace {

// The client is preferably the server's own executable: after a tmux upgrade
// it still speaks the protocol of the server that is running, which a newer
// client from PATH refuses to do. Inside a user namespace, which the unit's
// sandboxing creates, the kernel denies access to another process's exe link;
// then tmux from PATH is the client, as before.
std::string clientFor(pid_t server) {
    std::string exe = "/proc/" + std::to_string(server) + "/exe";
    return access(exe.c_str(), X_OK) == 0 ? exe : "tmux";
}

} // namespace

std::string Tmux::socketOf(pid_t server) {
    if (auto known = sockets_.find(server); known != sockets_.end()) {
        return known->second;
    }
    for (const auto &socket : socketsInDirectory()) {
        auto pid = runCommand({clientFor(server), "-S", socket, "display-message", "-p", "#{pid}"});
        if (pid && std::atoi(pid->c_str()) == server) {
            sockets_[server] = socket;
            return socket;
        }
    }
    return {};
}

std::optional<Tmux::Answer> Tmux::ask(dev_t tty, pid_t windowPid,
                                      const std::unordered_map<pid_t, Process> &processes) {
    auto server = serverOf(tty, processes);
    if (!server) {
        return std::nullopt;
    }
    std::string socket = socketOf(*server);
    if (socket.empty()) {
        return std::nullopt;
    }
    std::string client = clientFor(*server);
    auto panes = runCommand({client, "-S", socket, "list-panes", "-a", "-F", kTmuxPaneFormat});
    auto clients = runCommand({client, "-S", socket, "list-clients", "-F", kTmuxClientFormat});
    if (!panes || !clients) {
        sockets_.erase(*server); // the server went away or restarted
        return std::nullopt;
    }

    Answer answer;
    std::string terminal = terminalPath(tty);
    std::set<std::string> visibleSessions;
    for (const auto &pane : parseTmuxPanes(*panes)) {
        if (pane.terminal == terminal && pane.active) {
            visibleSessions.insert(pane.session);
        }
    }
    for (const auto &client : parseTmuxClients(*clients)) {
        answer.clientTerminals.insert(client.terminal);
        if (visibleSessions.contains(client.session) &&
            descendsFrom(client.pid, windowPid, processes)) {
            answer.visibleInWindow = true;
        }
    }
    return answer;
}
