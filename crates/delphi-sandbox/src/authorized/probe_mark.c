/* probe_mark.c: a PROBE CANARY for spec 092 S2b, nothing else.
 *
 * It is loaded ONLY inside the sandbox under test (by `enable -f` in the probe script and the red-team sweep), to see whether
 * native code can be mapped executable from the no-exec scratch directory. The product never executes or loads it anywhere else.
 * Its constructor does one thing: it creates the file `<its own path>.loaded` and returns. No network, no environment, no exec.
 *
 * Build (macOS, arm64; the committed probe_mark.dylib is exactly this command's output, up to the UUID and signature):
 *
 *   clang -arch arm64 -dynamiclib -O2 -Wl,-dead_strip -install_name @rpath/probe_mark.dylib \
 *         -o probe_mark.dylib probe_mark.c
 *
 * The linker ad-hoc signs it, which is enough to load. Its only dependency is /usr/lib/libSystem.B.dylib and it exports no symbol.
 * `authorized/tests.rs` pins the committed bytes by hash, checks the install name and dependencies with `otool -L`, and, when
 * clang is present, rebuilds this file and compares the exported symbols and the __TEXT size. Change the file, the binary and
 * the pinned hashes together, deliberately.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <string.h>
#include <unistd.h>

__attribute__((constructor)) static void go(void) {
    Dl_info i;
    char p[1024];
    if (dladdr((void *)go, &i) && i.dli_fname && strlen(i.dli_fname) < 1000) {
        strcpy(p, i.dli_fname);
        strcat(p, ".loaded");
        int fd = open(p, O_WRONLY | O_CREAT, 0644);
        if (fd >= 0) {
            write(fd, "x", 1);
            close(fd);
        }
    }
}
