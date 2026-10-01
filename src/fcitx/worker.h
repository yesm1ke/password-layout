#pragma once

#include <condition_variable>
#include <functional>
#include <mutex>
#include <thread>
#include <utility>

// Applies "Latin wanted / not wanted" on a thread of its own. fcitx5 handles
// every key press on one thread, so the addon must never wait for the
// compositor there; it only records the latest wish here, and this thread
// applies it. Wishes that change faster than they can be applied collapse into
// the last one, so hopping across fields costs at most one switch.
//
// Kept apart from the addon, with `apply` given in, so the tests can run it
// without fcitx5 or a compositor.
class Worker {
public:
    // `apply(latin)` runs on the worker thread only. It is called with false
    // first, to give back what a previous instance may have left held (a fcitx5
    // that died with a password field focused), and with false at the end.
    explicit Worker(std::function<void(bool)> apply)
        : apply_(std::move(apply)), thread_([this] { run(); }) {}

    ~Worker() {
        {
            std::lock_guard lock(mutex_);
            stopping_ = true;
            wanted_ = false; // give the layout back when fcitx5 exits
        }
        changed_.notify_one();
        thread_.join();
    }

    Worker(const Worker &) = delete;
    Worker &operator=(const Worker &) = delete;

    void want(bool latin) {
        {
            std::lock_guard lock(mutex_);
            if (wanted_ == latin) {
                return;
            }
            wanted_ = latin;
        }
        changed_.notify_one();
    }

private:
    void run() {
        apply_(false);
        bool applied = false;
        std::unique_lock lock(mutex_);
        for (;;) {
            changed_.wait(lock, [&] { return stopping_ || wanted_ != applied; });
            if (wanted_ != applied) {
                bool latin = wanted_;
                lock.unlock();
                apply_(latin);
                lock.lock();
                applied = latin;
            }
            if (stopping_ && wanted_ == applied) {
                return;
            }
        }
    }

    std::function<void(bool)> apply_;
    std::mutex mutex_;
    std::condition_variable changed_;
    bool wanted_ = false;
    bool stopping_ = false;
    std::thread thread_; // last: started once everything above exists
};
