#pragma once

#include <optional>
#include <string>
#include <string_view>
#include <vector>

// `password-layout doctor`: checks everything the project depends on and says,
// in plain words, what does not work and what to do about it.

struct Finding {
    enum class Level { Ok, Info, Warn, Fail };
    Level level = Level::Ok;
    std::string what;
    std::string hint; // what to do; empty when there is nothing to do
};

std::vector<Finding> diagnose();

// Prints the findings; returns 1 when any of them is a failure.
int printFindings(const std::vector<Finding> &findings);

// Helpers, kept apart for the tests.

// Compares dotted versions numerically, ignoring anything after the numbers
// ("5.1.15-1" < "5.1.16", "1.9.17p2" >= "1.9.14").
bool versionLess(std::string_view a, std::string_view b);

// The version in the first line of `sudo -V`, "Sudo version 1.9.17p2".
std::optional<std::string> sudoVersion(std::string_view output);

// The fcitx5 core version an addon description asks for ("0=core:5.1.0").
std::optional<std::string> addonRequiresCore(std::string_view conf);

// Whether a /proc/<pid>/maps text maps a library with this file name.
bool mapsLibrary(std::string_view maps, std::string_view fileName);
