#include "activity.h"

#include <cctype>
#include <dirent.h>
#include <poll.h>
#include <sys/inotify.h>
#include <unistd.h>

namespace {

constexpr const char *kPtsDirectory = "/dev/pts";

}

TtyActivity::TtyActivity() {
    fd_ = inotify_init1(IN_CLOEXEC | IN_NONBLOCK);
    if (fd_ < 0) {
        return;
    }
    directoryWatch_ = inotify_add_watch(fd_, kPtsDirectory, IN_CREATE);
    controllingTtyWatch_ = inotify_add_watch(fd_, "/dev/tty", IN_MODIFY);
    if (DIR *directory = opendir(kPtsDirectory)) {
        while (dirent *entry = readdir(directory)) {
            watch(entry->d_name);
        }
        closedir(directory);
    }
}

TtyActivity::~TtyActivity() {
    if (fd_ >= 0) {
        close(fd_);
    }
}

void TtyActivity::watch(const char *name) {
    if (!std::isdigit(static_cast<unsigned char>(name[0]))) {
        return;
    }
    // Fails for other users' terminals, which cannot be inspected anyway.
    std::string path = std::string(kPtsDirectory) + "/" + name;
    int descriptor = inotify_add_watch(fd_, path.c_str(), IN_MODIFY);
    if (descriptor >= 0) {
        watched_[descriptor] = path;
        active_.insert(std::move(path));
    }
}

bool TtyActivity::wait(int timeoutMs) {
    pollfd descriptor{fd_, POLLIN, 0};
    // With no inotify the descriptor is negative, which poll() ignores: the
    // call degrades to a plain sleep and the caller falls back to polling.
    if (poll(&descriptor, 1, timeoutMs) <= 0) {
        return false;
    }
    return drain();
}

bool TtyActivity::drain() {
    bool active = false;
    alignas(inotify_event) char buffer[4096];
    for (;;) {
        ssize_t count = read(fd_, buffer, sizeof(buffer));
        if (count <= 0) {
            break;
        }
        active = true;
        for (char *at = buffer; at < buffer + count;) {
            auto *event = reinterpret_cast<inotify_event *>(at);
            if ((event->mask & IN_Q_OVERFLOW) || event->wd == controllingTtyWatch_) {
                // Events were lost, or the output went through /dev/tty: any
                // terminal may have printed.
                for (const auto &[descriptor, path] : watched_) {
                    active_.insert(path);
                }
            } else if (event->wd == directoryWatch_) {
                if ((event->mask & IN_CREATE) && event->len) {
                    watch(event->name);
                }
            } else if (auto found = watched_.find(event->wd); found != watched_.end()) {
                if (event->mask & IN_IGNORED) {
                    // The terminal is gone and the kernel dropped its watch.
                    active_.erase(found->second);
                    watched_.erase(found);
                } else {
                    active_.insert(found->second);
                }
            }
            at += sizeof(inotify_event) + event->len;
        }
        if (static_cast<size_t>(count) < sizeof(buffer) / 2) {
            break; // a short read means the queue is empty
        }
    }
    return active;
}

std::set<std::string> TtyActivity::takeActive() {
    std::set<std::string> taken;
    taken.swap(active_);
    return taken;
}

std::set<std::string> TtyActivity::all() const {
    std::set<std::string> paths;
    for (const auto &[descriptor, path] : watched_) {
        paths.insert(path);
    }
    return paths;
}
