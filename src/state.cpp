#include "state.h"

#include <algorithm>
#include <cerrno>
#include <cstdlib>
#include <fcntl.h>
#include <fstream>
#include <stdexcept>
#include <sys/file.h>
#include <sys/stat.h>
#include <system_error>
#include <unistd.h>

namespace {

// Held for the lifetime of the object; serializes enter() and leave() across
// processes.
class Lock {
public:
    explicit Lock(const std::string &directory) {
        if (mkdir(directory.c_str(), 0700) < 0 && errno != EEXIST) {
            throw std::system_error(errno, std::generic_category(), directory);
        }
        std::string path = directory + "/lock";
        fd_ = open(path.c_str(), O_RDWR | O_CREAT | O_CLOEXEC, 0600);
        if (fd_ < 0 || flock(fd_, LOCK_EX) < 0) {
            throw std::system_error(errno, std::generic_category(), path);
        }
    }
    ~Lock() { close(fd_); }
    Lock(const Lock &) = delete;
    Lock &operator=(const Lock &) = delete;

private:
    int fd_ = -1;
};

} // namespace

State::State(std::string directory) : directory_(std::move(directory)) {
    if (directory_.empty()) {
        const char *runtime = std::getenv("XDG_RUNTIME_DIR");
        if (!runtime || !*runtime) {
            throw std::runtime_error("XDG_RUNTIME_DIR is not set");
        }
        directory_ = std::string(runtime) + "/password-layout";
    }
    path_ = directory_ + "/state";
}

// First line is the layout to restore, every following line one holder.
State::Contents State::load() const {
    Contents contents;
    std::ifstream file(path_);
    std::string line;
    if (!std::getline(file, line)) {
        return contents;
    }
    contents.previous = std::atoi(line.c_str());
    while (std::getline(file, line)) {
        if (!line.empty()) {
            contents.holders.push_back(line);
        }
    }
    return contents;
}

void State::save(const Contents &contents) const {
    std::ofstream file(path_, std::ios::trunc);
    file << contents.previous << '\n';
    for (const auto &holder : contents.holders) {
        file << holder << '\n';
    }
}

void State::enter(const std::string &holder, Compositor &compositor) {
    Lock lock(directory_);
    Contents contents = load();
    if (contents.holders.empty()) {
        contents.previous = compositor.layoutIndex();
        if (contents.previous != kLatinIndex) {
            compositor.setLayout(kLatinIndex);
        }
    }
    if (std::ranges::find(contents.holders, holder) == contents.holders.end()) {
        contents.holders.push_back(holder);
    }
    save(contents);
}

void State::leave(const std::string &holder, Compositor &compositor) {
    Lock lock(directory_);
    Contents contents = load();
    auto found = std::ranges::find(contents.holders, holder);
    if (found == contents.holders.end()) {
        return;
    }
    contents.holders.erase(found);
    if (!contents.holders.empty()) {
        save(contents);
        return;
    }
    unlink(path_.c_str());
    if (contents.previous != kLatinIndex) {
        compositor.setLayout(contents.previous);
    }
}

std::vector<std::string> State::holders() {
    Lock lock(directory_);
    return load().holders;
}
