// Where one pass over the terminals spends its time when it runs cold, right
// after a 200 ms sleep, the way the polling loop ran it. Each reading of the
// clock costs a few microseconds itself; that overhead is printed so it can be
// subtracted (once for the directory, once per terminal for the other rows).

#include <chrono>
#include <cstdio>
#include <dirent.h>
#include <fcntl.h>
#include <string>
#include <sys/stat.h>
#include <termios.h>
#include <thread>
#include <time.h>
#include <unistd.h>
#include <vector>

static double cpuMicroseconds() {
    timespec now;
    clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &now);
    return now.tv_sec * 1e6 + now.tv_nsec / 1e3;
}

int main() {
    const int ticks = 100;
    double clock = 0, list = 0, stats = 0, opens = 0, modes = 0, closes = 0;
    size_t terminals = 0;
    for (int i = 0; i < ticks; ++i) {
        std::this_thread::sleep_for(std::chrono::milliseconds(200));
        double c0 = cpuMicroseconds();
        double c1 = cpuMicroseconds();
        clock += c1 - c0;

        double before = cpuMicroseconds();
        std::vector<std::string> paths;
        DIR *directory = opendir("/dev/pts");
        while (dirent *entry = readdir(directory)) {
            if (entry->d_name[0] >= '0' && entry->d_name[0] <= '9') {
                paths.push_back(std::string("/dev/pts/") + entry->d_name);
            }
        }
        closedir(directory);
        list += cpuMicroseconds() - before;
        terminals = paths.size();

        for (const auto &path : paths) {
            double t0 = cpuMicroseconds();
            struct stat info;
            stat(path.c_str(), &info);
            double t1 = cpuMicroseconds();
            int fd = open(path.c_str(), O_RDONLY | O_NOCTTY | O_NONBLOCK | O_CLOEXEC);
            double t2 = cpuMicroseconds();
            termios settings;
            if (fd >= 0) {
                tcgetattr(fd, &settings);
            }
            double t3 = cpuMicroseconds();
            if (fd >= 0) {
                close(fd);
            }
            double t4 = cpuMicroseconds();
            stats += t1 - t0;
            opens += t2 - t1;
            modes += t3 - t2;
            closes += t4 - t3;
        }
    }
    std::printf("%zu terminals, per pass; one clock reading costs %.1f us\n", terminals,
                clock / ticks);
    std::printf("  list /dev/pts  %6.1f us\n", list / ticks);
    std::printf("  stat           %6.1f us\n", stats / ticks);
    std::printf("  open           %6.1f us\n", opens / ticks);
    std::printf("  tcgetattr      %6.1f us\n", modes / ticks);
    std::printf("  close          %6.1f us\n", closes / ticks);
}
