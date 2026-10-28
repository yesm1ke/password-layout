// fcitx5 addon shim: Latin keyboard layout while a password field has focus.
//
// This is the C++ shim of the Rust addon: only the part that needs fcitx5's
// C++ API (the factory, the event watchers, the field test). The worker
// thread, the Hyprland client and the state file are Rust - the same code
// the service uses (rust/password-layout-addon/src; the design in
// the wiki page "Rust rewrite"). Copied from
// src/fcitx/passwordlayout.cpp, the C++ reference.
//
// Applications tell the input method what kind of field has focus. Browsers
// and toolkits mark password fields with the Password capability (Qt Quick,
// since Qt 6.12, with a set of others; see isPasswordField), which is the
// same fact macOS acts on when it turns on secure input. The addon follows
// focus and that flag and asks the Rust worker when Latin is needed, under
// the holder name "im".
//
// fcitx5 runs everything on one thread, and every key press goes through it,
// so the addon never talks to the compositor there: a worker thread does, and
// only ever applies the latest wish. Focus hopping quickly across fields then
// costs at most one switch, and a slow compositor never delays typing.

#include "password_layout_addon.h"

#include <cstdio>
#include <exception>
#include <memory>
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

// Owns the Rust worker; stopping it gives the layout back (see Worker in
// rust/password-layout-addon/src/worker.rs).
class WorkerHandle {
public:
    WorkerHandle() : worker_(password_layout_addon_start()) {}
    ~WorkerHandle() { password_layout_addon_stop(worker_); }
    WorkerHandle(const WorkerHandle &) = delete;
    WorkerHandle &operator=(const WorkerHandle &) = delete;
    void want(bool latin) { password_layout_addon_want(worker_, latin); }

private:
    PasswordLayoutWorker *worker_;
};

// Since Qt 6.12, a Qt Quick field with echoMode Password no longer carries the
// Password capability (QTBUG-88367), only Sensitive, NoSpellCheck and
// NoAutoUpperCase. Sensitive alone is not enough: Chromium sets it on every
// field of an incognito window, through a frontend that never sets the other
// two.
bool isPasswordField(fcitx::CapabilityFlags flags) {
    using fcitx::CapabilityFlag;
    return flags.test(CapabilityFlag::Password) ||
           (flags.test(CapabilityFlag::Sensitive) && flags.test(CapabilityFlag::NoSpellCheck) &&
            flags.test(CapabilityFlag::NoAutoUpperCase));
}

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
                               << " password=" << isPasswordField(ic->capabilityFlags());
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
        worker_.want(isPasswordField(ic->capabilityFlags()));
    }

    fcitx::InputContext *focused_ = nullptr;
    WorkerHandle worker_;
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
