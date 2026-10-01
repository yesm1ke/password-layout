// fcitx5 addon: Latin keyboard layout while a password field has focus.
//
// Applications tell the input method what kind of field has focus. Browsers
// and toolkits mark password fields with the Password capability, which is the
// same fact macOS acts on when it turns on secure input. This addon follows
// focus and that flag and tells the shared state (src/state.*) when Latin is
// needed, under the holder name "im".
//
// fcitx5 runs everything on one thread, and every key press goes through it,
// so the addon never talks to the compositor there: a worker thread does, and
// only ever applies the latest wish. Focus hopping quickly across fields then
// costs at most one switch, and a slow compositor never delays typing.

#include "compositor.h"
#include "state.h"

#include <condition_variable>
#include <cstdio>
#include <exception>
#include <memory>
#include <mutex>
#include <thread>
#include <vector>

#include <fcitx-utils/capabilityflags.h>
#include <fcitx/addonfactory.h>
#include <fcitx/addoninstance.h>
#include <fcitx/addonmanager.h>
#include <fcitx/event.h>
#include <fcitx/inputcontext.h>
#include <fcitx/instance.h>
#include <fcitx-utils/log.h>

// Debug output, off by default. Turn on at runtime with
//   busctl --user call org.fcitx.Fcitx5 /controller org.fcitx.Fcitx.Controller1 SetLogRule s passwordlayout=5
FCITX_DEFINE_LOG_CATEGORY(passwordlayoutLog, "passwordlayout")
#define PASSWORDLAYOUT_DEBUG() FCITX_LOGC(passwordlayoutLog, Debug)

namespace {

const std::string kHolder = "im";

class Worker {
public:
    Worker() : thread_([this] { run(); }) {}

    ~Worker() {
        {
            std::lock_guard lock(mutex_);
            stopping_ = true;
            wanted_ = false; // give the layout back when fcitx5 exits
        }
        changed_.notify_one();
        thread_.join();
    }

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
        // A previous fcitx5 that died while a password field had focus left
        // "im" in the shared state, and while it is there the terminal service
        // cannot switch either. Give it back first, on this thread like every
        // other call that reaches the compositor.
        apply(false);
        bool applied = false;
        std::unique_lock lock(mutex_);
        for (;;) {
            changed_.wait(lock, [&] { return stopping_ || wanted_ != applied; });
            if (wanted_ != applied) {
                bool latin = wanted_;
                lock.unlock();
                apply(latin);
                lock.lock();
                applied = latin;
            }
            if (stopping_ && wanted_ == applied) {
                return;
            }
        }
    }

    static void apply(bool latin) {
        try {
            Hyprland compositor;
            State state;
            if (latin) {
                state.enter(kHolder, compositor);
            } else {
                state.leave(kHolder, compositor);
            }
        } catch (const std::exception &error) {
            // Outside Hyprland, or the compositor is restarting: nothing to do.
            std::fprintf(stderr, "passwordlayout: %s\n", error.what());
        }
    }

    std::mutex mutex_;
    std::condition_variable changed_;
    bool wanted_ = false;
    bool stopping_ = false;
    std::thread thread_;
};

class PasswordLayout : public fcitx::AddonInstance {
public:
    explicit PasswordLayout(fcitx::Instance *instance) {
        using fcitx::EventType;
        auto follow = [this](EventType type) {
            return [this, type](fcitx::Event &event) {
                auto *ic = static_cast<fcitx::InputContextEvent &>(event).inputContext();
                update(ic, type == EventType::InputContextFocusOut);
            };
        };
        for (EventType type : {EventType::InputContextFocusIn, EventType::InputContextFocusOut,
                               EventType::InputContextCapabilityChanged}) {
            watchers_.push_back(instance->watchEvent(type, fcitx::EventWatcherPhase::Default,
                                                     follow(type)));
        }
    }

private:
    void update(fcitx::InputContext *ic, bool leaving) {
        PASSWORDLAYOUT_DEBUG() << (leaving ? "focus out" : "focus in or capability change")
                               << " program=" << ic->program() << " focus=" << ic->hasFocus()
                               << " password="
                               << ic->capabilityFlags().test(fcitx::CapabilityFlag::Password);
        if (leaving) {
            // Focus moving between fields sends a focus-out before the next
            // focus-in; the worker only acts on where it ends up.
            if (ic == focused_) {
                focused_ = nullptr;
                worker_.want(false);
            }
            return;
        }
        if (!ic->hasFocus()) {
            return; // a capability change of a field in the background
        }
        focused_ = ic;
        worker_.want(ic->capabilityFlags().test(fcitx::CapabilityFlag::Password));
    }

    fcitx::InputContext *focused_ = nullptr;
    Worker worker_;
    std::vector<std::unique_ptr<fcitx::HandlerTableEntry<fcitx::EventHandler>>> watchers_;
};

class PasswordLayoutFactory : public fcitx::AddonFactory {
public:
    fcitx::AddonInstance *create(fcitx::AddonManager *manager) override {
        return new PasswordLayout(manager->instance());
    }
};

} // namespace

FCITX_ADDON_FACTORY_V2(passwordlayout, PasswordLayoutFactory)
