// Content-free process counters for idle-perf.ts, on macOS only.
#include <libproc.h>
#include <mach/mach_time.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/resource.h>

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    int pid = atoi(argv[1]);
    struct rusage_info_v4 usage = {0};
    struct proc_taskinfo task = {0};
    if (proc_pid_rusage(pid, RUSAGE_INFO_V4, (rusage_info_t *)&usage) != 0 ||
        proc_pidinfo(pid, PROC_PIDTASKINFO, 0, &task, sizeof(task)) != sizeof(task)) return 1;
    mach_timebase_info_data_t timebase;
    mach_timebase_info(&timebase);
    // rusage_info times are Mach ticks, not nanoseconds (on Apple Silicon
    // they differ by about 42 times).
    unsigned long long cpu_ns = (unsigned long long)((long double)(usage.ri_user_time + usage.ri_system_time) * timebase.numer / timebase.denom);
    printf("{\"cpuNs\":%llu,\"interruptWakeups\":%llu,\"idleWakeups\":%llu,\"contextSwitches\":%u}\n",
        cpu_ns, usage.ri_interrupt_wkups,
        usage.ri_pkg_idle_wkups, task.pti_csw);
    return 0;
}
