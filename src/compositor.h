#pragma once

#include <optional>
#include <string>
#include <string_view>
#include <sys/types.h>
#include <vector>

// What the rest of the program needs from the window manager.
class Compositor {
public:
    virtual ~Compositor() = default;
    virtual int layoutIndex() = 0;
    // The layout to type passwords in; nothing when no layout is Latin.
    virtual std::optional<int> latinIndex() = 0;
    virtual void setLayout(int index) = 0;
    // 0 when no window has focus.
    virtual pid_t focusedPid() = 0;
};

// Talks to Hyprland over its request socket. Throws std::system_error when the
// compositor cannot be reached.
class Hyprland : public Compositor {
public:
    Hyprland();
    int layoutIndex() override;
    std::optional<int> latinIndex() override;
    void setLayout(int index) override;
    pid_t focusedPid() override;

private:
    std::string request(std::string_view command) const;
    std::string socketPath_;
};

// Every integer stored under `key` in a JSON document, in order of appearance.
// Hyprland's replies are flat enough that this is all the parsing they need: a
// quote inside a string value is always escaped, so `"key": ` can only match a
// real key.
std::vector<long> jsonIntegers(std::string_view json, std::string_view key);

// Every string stored under `key`, unescaped, in order of appearance.
std::vector<std::string> jsonStrings(std::string_view json, std::string_view key);

// Index of the first Latin layout in xkb's comma-separated lists, e.g.
// ("ru,us", ",") -> 1. A layout counts as non-Latin by its code, from the same
// list Omarchy uses to decide whether to put "us" first; a variant with
// "latin" in its name (rs(latin)) makes it Latin again.
std::optional<int> firstLatinLayout(std::string_view layouts, std::string_view variants);

// The value most of them agree on; `fallback` when there are none.
int mostCommon(const std::vector<long> &values, int fallback);
