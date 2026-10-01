// CPU cost of one wakeup of the plain polling loop: sleep 200 ms, then scan
// every terminal. Run it while the installed service is under the same load and
// compare against the service's own figures (see https://github.com/yesm1ke/password-layout/wiki/Resource-use).

#include "tty.h"

#include <chrono>
#include <cstdio>
#include <thread>
#include <time.h>
#include <unistd.h>

static double cpuMicroseconds() {
    timespec now;
    clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &now);
    return now.tv_sec * 1e6 + now.tv_nsec / 1e3;
}

int main() {
    const int ticks = 150;
    double scan = 0;
    double start = cpuMicroseconds();
    for (int i = 0; i < ticks; ++i) {
        std::this_thread::sleep_for(std::chrono::milliseconds(200));
        double before = cpuMicroseconds();
        passwordPtys(getuid());
        scan += cpuMicroseconds() - before;
    }
    double total = cpuMicroseconds() - start;
    std::printf("polling, per wakeup: total %.1f us | scan %.1f us | rest %.1f us\n", total / ticks,
                scan / ticks, (total - scan) / ticks);
}
