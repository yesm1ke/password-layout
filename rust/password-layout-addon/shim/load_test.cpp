// Loads the built addon without a running fcitx5: it must link, load with
// every symbol resolved, export the fcitx5 factory and keep the Rust symbols
// to itself. The factory is not called (that needs a running fcitx5
// AddonManager).
#include <dlfcn.h>
#include <cstdio>

int main(int argc, char **argv) {
    if (argc != 2) {
        std::fprintf(stderr, "usage: %s libpasswordlayout.so\n", argv[0]);
        return 1;
    }
    void *handle = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    if (!handle) {
        std::fprintf(stderr, "dlopen: %s\n", dlerror());
        return 1;
    }
    if (!dlsym(handle, "fcitx_addon_factory_instance_passwordlayout")) {
        std::fprintf(stderr, "the fcitx5 factory is not exported\n");
        return 1;
    }
    if (dlsym(handle, "password_layout_addon_start")) {
        std::fprintf(stderr, "the Rust symbols must not be exported\n");
        return 1;
    }
    std::printf("load test: ok\n");
    return 0;
}
