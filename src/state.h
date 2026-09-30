#pragma once

#include "compositor.h"

#include <optional>
#include <string>
#include <vector>

// Who currently wants Latin, and which layout to give back afterwards.
//
// Kept in a file because the sources that notice password prompts are separate
// processes. The layout active before the first enter() returns after the last
// leave().
class State {
public:
    // Defaults to $XDG_RUNTIME_DIR/password-layout.
    explicit State(std::string directory = {});

    // Like macOS with a secure field: nothing happens when the active layout is
    // already Latin; otherwise the last Latin layout in use is chosen (the
    // first Latin one when none has been seen), and the previous layout comes
    // back after the last leave().
    void enter(const std::string &holder, Compositor &compositor);
    void leave(const std::string &holder, Compositor &compositor);
    std::vector<std::string> holders();

    // Remembers the active layout if it is Latin, as the one to use for the
    // next password. Called whenever the layout changes.
    void noteLayout(Compositor &compositor);
    std::optional<int> lastLatin() const;

private:
    struct Contents {
        int previous = -1; // the layout to give back; -1 when Latin was not switched to
        std::vector<std::string> holders;
    };

    Contents load() const;
    void save(const Contents &contents) const;

    std::string directory_;
    std::string path_;
    std::string lastLatinPath_;
};
