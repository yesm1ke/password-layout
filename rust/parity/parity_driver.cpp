// The C++ side of the Rust parity check (stage 1c). Same command line and
// same output as rust/password-layout-core/examples/parity_driver.rs:
//
//   parity_driver classify
//       stdin: lines "LAYOUTS<TAB>VARIANTS" (VARIANTS may be empty)
//       stdout: per line the result of latinLayouts as 0/1 characters, e.g. "10"
//   parity_driver run DIR INDEX LATIN OP...
//       DIR exists; INDEX is the starting layout; LATIN like "10"
//       (empty string = no layouts)
//       OP is one of: enter:NAME  leave:NAME  note  set:N (user switches by
//       hand)  latin:BITS (config changed)
//       after every OP print one line: "OP ok index=I switches=A,B" or
//       "OP error" (switches = all set_layout calls so far, comma-separated,
//       empty when none)
//   parity_driver json KEY...
//       stdin: read in whole, as bytes; for each KEY one line "int KEY" and
//       one line " V" per value of jsonIntegers, one line "str KEY" and one
//       line " [S]" per value of jsonStrings (raw bytes); then the line
//       "keyboard [LAYOUTS] [VARIANTS]" (keyboardLayouts of the whole
//       input) and "most-common N", N = mostCommon(jsonIntegers(input,
//       first KEY), -1)
//   parity_driver hyprland RUNTIME SIGNATURE OP...
//       RUNTIME is a runtime directory; SIGNATURE a Hyprland instance
//       signature (both set in the environment before the client is
//       constructed). OP is one of: index  latin  focused  list  set:N
//       after every OP print one line: "OP VALUE" (latin as 0/1 characters,
//       set:N "ok") or "OP error"
//       (there is no `serve`: the fake Hyprland is Rust-only)
//
// Built with g++ against src/state.cpp and src/compositor.cpp; see run.sh.

#include "compositor.h"
#include "state.h"

#include <cstdio>
#include <cstdlib>
#include <exception>
#include <iostream>
#include <optional>
#include <iterator>
#include <string>
#include <vector>

namespace {

// Same as FakeCompositor in tests/test.cpp.
class FakeCompositor : public Compositor {
public:
    FakeCompositor(int start, std::vector<bool> latinLayouts)
        : index(start), latin(std::move(latinLayouts)) {}
    int layoutIndex() override { return index; }
    std::vector<bool> latinLayouts() override { return latin; }
    void setLayout(int to) override {
        index = to;
        switches.push_back(to);
    }
    pid_t focusedPid() override { return 0; }

    int index;
    std::vector<bool> latin;
    std::vector<int> switches;
};

std::vector<bool> bits(const std::string &text) {
    std::vector<bool> result;
    for (char c : text) {
        result.push_back(c == '1');
    }
    return result;
}

int classify() {
    std::string line;
    while (std::getline(std::cin, line)) {
        auto tab = line.find('\t');
        auto layouts = line.substr(0, tab);
        auto variants = tab == std::string::npos ? "" : line.substr(tab + 1);
        auto latin = latinLayouts(layouts, variants);
        for (bool isLatin : latin) {
            std::printf("%c", isLatin ? '1' : '0');
        }
        std::printf("\n");
    }
    return 0;
}

int run(const std::string &directory, int start, const std::string &latin,
        const std::vector<std::string> &operations) {
    FakeCompositor compositor(start, bits(latin));
    State state(directory);
    for (const auto &operation : operations) {
        bool ok = true;
        try {
            if (operation == "note") {
                state.noteLayout(compositor);
            } else if (operation.rfind("enter:", 0) == 0) {
                state.enter(operation.substr(6), compositor);
            } else if (operation.rfind("leave:", 0) == 0) {
                state.leave(operation.substr(6), compositor);
            } else if (operation.rfind("set:", 0) == 0) {
                compositor.index = std::stoi(operation.substr(4));
            } else if (operation.rfind("latin:", 0) == 0) {
                compositor.latin = bits(operation.substr(6));
            } else {
                ok = false;
            }
        } catch (const std::exception &) {
            ok = false;
        }
        if (!ok) {
            std::printf("%s error\n", operation.c_str());
            continue;
        }
        std::string switches;
        for (int index : compositor.switches) {
            if (!switches.empty()) {
                switches += ',';
            }
            switches += std::to_string(index);
        }
        std::printf("%s ok index=%d switches=%s\n", operation.c_str(), compositor.index,
                    switches.c_str());
    }
    return 0;
}

int json(const std::vector<std::string> &keys) {
    std::string input((std::istreambuf_iterator<char>(std::cin)),
                      std::istreambuf_iterator<char>());
    for (const auto &key : keys) {
        std::cout << "int " << key << '\n';
        for (long value : jsonIntegers(input, key)) {
            std::cout << ' ' << value << '\n';
        }
        std::cout << "str " << key << '\n';
        for (const std::string &value : jsonStrings(input, key)) {
            std::cout.write(value.data(), value.size());
            std::cout << '\n';
        }
    }
    auto [layouts, variants] = keyboardLayouts(input);
    std::cout << "keyboard " << layouts << ' ' << variants << '\n';
    std::cout << "most-common " << mostCommon(jsonIntegers(input, keys.front()), -1) << '\n';
    return 0;
}

// One Hyprland client, a fresh request per OP; a failed request prints
// "OP error" and the next OP goes on.
int hyprland(const std::string &runtime, const std::string &signature,
             const std::vector<std::string> &operations) {
    // The client reads both from the environment; set them before it is
    // constructed.
    setenv("XDG_RUNTIME_DIR", runtime.c_str(), 1);
    setenv("HYPRLAND_INSTANCE_SIGNATURE", signature.c_str(), 1);
    Hyprland compositor;
    for (const auto &operation : operations) {
        std::optional<std::string> value;
        try {
            if (operation == "index") {
                value = std::to_string(compositor.layoutIndex());
            } else if (operation == "latin") {
                std::string bits;
                for (bool isLatin : compositor.latinLayouts()) {
                    bits += isLatin ? '1' : '0';
                }
                value = bits;
            } else if (operation == "focused") {
                value = std::to_string(compositor.focusedPid());
            } else if (operation == "list") {
                value = compositor.layoutList();
            } else if (operation.rfind("set:", 0) == 0) {
                compositor.setLayout(std::stoi(operation.substr(4)));
                value = "ok";
            }
        } catch (const std::exception &) {
            value.reset();
        }
        if (!value) {
            std::printf("%s error\n", operation.c_str());
            continue;
        }
        std::printf("%s %s\n", operation.c_str(), value->c_str());
    }
    return 0;
}

} // namespace

int main(int argc, char **argv) {
    static const char *usage =
        "usage: parity_driver classify | run DIR INDEX LATIN OP... | "
        "json KEY... | hyprland RUNTIME SIGNATURE OP...\n";
    if (argc < 2) {
        std::fprintf(stderr, "%s", usage);
        return 2;
    }
    if (std::string_view(argv[1]) == "classify") {
        return classify();
    }
    if (std::string_view(argv[1]) == "run" && argc >= 5) {
        return run(argv[2], std::stoi(argv[3]), argv[4],
                   std::vector<std::string>(argv + 5, argv + argc));
    }
    if (std::string_view(argv[1]) == "json" && argc >= 3) {
        return json(std::vector<std::string>(argv + 2, argv + argc));
    }
    if (std::string_view(argv[1]) == "hyprland" && argc >= 4) {
        return hyprland(argv[2], argv[3],
                        std::vector<std::string>(argv + 4, argv + argc));
    }
    std::fprintf(stderr, "%s", usage);
    return 2;
}
