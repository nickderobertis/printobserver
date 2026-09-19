/*
 * What one invocation's own process tree did, recorded from inside it.
 *
 * This is the command-line tier's observation where `strace` is not there to
 * make it: loaded into the program under test (and, inherited through the
 * environment, into every child it starts), it records every `connect` any of
 * them makes and, given paths to watch, every file access to one of those
 * paths, failing each with EACCES as a user who may not read them would be
 * failed. `interposer.rs` builds it, loads it and reads what it wrote.
 *
 * `PRINTOBSERVER_INTERPOSE_SELF`, when set, confines it to the process it was
 * loaded into: the loader variable is taken out of that process's environment
 * before it starts anything, so its children carry no recorder.
 *
 * Three kinds of line are written, one `write` each, appended to the file
 * `PRINTOBSERVER_INTERPOSE_LOG` names:
 *
 *   loaded <pid>                       this library is in process <pid>
 *   connected <address>:<port>         a connect to an internet endpoint
 *   <call> "<path>" = -1 EACCES        an access to a watched path, refused
 *
 * `PRINTOBSERVER_INTERPOSE_WATCH` names the watched paths, one per line; an
 * access to one of them or to anything beneath one is refused.
 *
 * On macOS the wrappers are installed through dyld's `__DATA,__interpose`
 * table, which rebinds every other image's references to the named functions
 * and leaves this library's own calls reaching the originals. On Linux the
 * same wrappers are exported under the functions' own names and loaded with
 * `LD_PRELOAD`, calling through to the next definition; that build is what
 * lets the recording half be proven on a Linux host as well.
 *
 * The functions wrapped are the ones Rust's standard library and the `libc`
 * crate reach for these operations on each platform: `open`, `openat`, `stat`,
 * `lstat`, `fstatat`, `access`, `faccessat`, `opendir` and `connect`, plus on
 * Linux the large-file and `statx` spellings glibc links them under. The
 * table below names each by its C spelling, so it interposes whatever symbol
 * the SDK's own headers resolve that name to — on Apple silicon, the one
 * supported macOS, that is the plain symbol.
 */

#ifndef __APPLE__
#define _GNU_SOURCE
#endif

#include <arpa/inet.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <netinet/in.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

#ifdef __APPLE__
#include <sys/param.h>
/* The original, reached directly: dyld does not rebind this image's own calls. */
#define REAL(name) name
/* A wrapper is this library's own, and reached only through the table. */
#define WRAPPER static
#define WRAP(name) po_##name
#define INTERPOSE(name)                                                        \
    __attribute__((used)) static struct {                                      \
        const void *replacement;                                               \
        const void *replacee;                                                  \
    } po_interpose_##name __attribute__((section("__DATA,__interpose"))) = {   \
        (const void *)(unsigned long)&po_##name,                               \
        (const void *)(unsigned long)&name,                                    \
    };
#else
#include <dlfcn.h>
/* The next definition after this library's own, which is the C library's. */
static void *po_next(const char *name) {
    void *found = dlsym(RTLD_NEXT, name);
    if (found == NULL) {
        abort();
    }
    return found;
}
#define REAL(name) ((__typeof__(&name))po_next(#name))
#define WRAPPER __attribute__((visibility("default")))
#define WRAP(name) name
#endif

#define PO_LOG_ENV "PRINTOBSERVER_INTERPOSE_LOG"
#define PO_WATCH_ENV "PRINTOBSERVER_INTERPOSE_WATCH"
/* Set, this process alone is observed: see po_loaded. */
#define PO_SELF_ENV "PRINTOBSERVER_INTERPOSE_SELF"
#ifdef __APPLE__
#define PO_LOADER_ENV "DYLD_INSERT_LIBRARIES"
#else
#define PO_LOADER_ENV "LD_PRELOAD"
#endif
#define PO_PATH_CAP 4096

/* Append one line to the log, leaving errno as the caller had it. */
static void po_log(const char *line, size_t length) {
    int saved = errno;
    const char *log = getenv(PO_LOG_ENV);
    if (log != NULL && *log != '\0') {
        int fd = REAL(open)(log, O_WRONLY | O_APPEND | O_CREAT | O_CLOEXEC, 0600);
        if (fd >= 0) {
            ssize_t written = write(fd, line, length);
            (void)written;
            close(fd);
        }
    }
    errno = saved;
}

/*
 * Asked to observe this process alone, the library takes its own loader
 * variable out of the environment here, before the program has started
 * anything, so nothing it starts inherits the library. A journey whose subject
 * is one program's own calls asks for this: a child of another ABI — the
 * system shell on Apple Silicon is arm64e where this library is arm64 — would
 * otherwise be killed by the loader for a library it cannot take.
 */
__attribute__((constructor)) static void po_loaded(void) {
    char line[64];
    if (getenv(PO_SELF_ENV) != NULL) {
        unsetenv(PO_LOADER_ENV);
    }
    int length = snprintf(line, sizeof line, "loaded %ld\n", (long)getpid());
    if (length > 0 && (size_t)length < sizeof line) {
        po_log(line, (size_t)length);
    }
}

/* The absolute path one call names, relative paths resolved against their directory. */
static int po_resolve(int dirfd, const char *path, char *out, size_t cap) {
    if (path == NULL || *path == '\0') {
        return 0;
    }
    if (*path == '/') {
        int length = snprintf(out, cap, "%s", path);
        return length > 0 && (size_t)length < cap;
    }
    char directory[PO_PATH_CAP];
    if (dirfd == AT_FDCWD) {
        if (getcwd(directory, sizeof directory) == NULL) {
            return 0;
        }
    } else {
#ifdef __APPLE__
        char named[MAXPATHLEN];
        if (fcntl(dirfd, F_GETPATH, named) == -1) {
            return 0;
        }
        snprintf(directory, sizeof directory, "%s", named);
#else
        char link[64];
        snprintf(link, sizeof link, "/proc/self/fd/%d", dirfd);
        ssize_t length = readlink(link, directory, sizeof directory - 1);
        if (length <= 0) {
            return 0;
        }
        directory[length] = '\0';
#endif
    }
    int length = snprintf(out, cap, "%s/%s", directory, path);
    return length > 0 && (size_t)length < cap;
}

/* Whether a path is one of the watched ones, or beneath one. */
static int po_watched(const char *path) {
    const char *watching = getenv(PO_WATCH_ENV);
    if (watching == NULL) {
        return 0;
    }
    const char *entry = watching;
    while (*entry != '\0') {
        const char *end = strchr(entry, '\n');
        size_t length = end == NULL ? strlen(entry) : (size_t)(end - entry);
        while (length > 1 && entry[length - 1] == '/') {
            length--;
        }
        if (length > 0 && strncmp(path, entry, length) == 0 &&
            (path[length] == '\0' || path[length] == '/')) {
            return 1;
        }
        if (end == NULL) {
            break;
        }
        entry = end + 1;
    }
    return 0;
}

/* Refuse one call on a watched path, recording it; answer whether it was refused. */
static int po_refused(const char *call, int dirfd, const char *path) {
    char resolved[PO_PATH_CAP];
    if (!po_resolve(dirfd, path, resolved, sizeof resolved) || !po_watched(resolved)) {
        return 0;
    }
    char line[PO_PATH_CAP + 64];
    int length = snprintf(line, sizeof line, "%s \"%s\" = -1 EACCES\n", call, resolved);
    if (length > 0 && (size_t)length < sizeof line) {
        po_log(line, (size_t)length);
    }
    errno = EACCES;
    return 1;
}

/* Record the internet endpoint one connect names. */
static void po_connected(const struct sockaddr *address) {
    char host[INET6_ADDRSTRLEN];
    char line[INET6_ADDRSTRLEN + 32];
    int length = 0;
    if (address == NULL) {
        return;
    }
    if (address->sa_family == AF_INET) {
        const struct sockaddr_in *v4 = (const struct sockaddr_in *)address;
        if (inet_ntop(AF_INET, &v4->sin_addr, host, sizeof host) == NULL) {
            return;
        }
        length = snprintf(line, sizeof line, "connected %s:%u\n", host,
                          (unsigned)ntohs(v4->sin_port));
    } else if (address->sa_family == AF_INET6) {
        const struct sockaddr_in6 *v6 = (const struct sockaddr_in6 *)address;
        if (inet_ntop(AF_INET6, &v6->sin6_addr, host, sizeof host) == NULL) {
            return;
        }
        length = snprintf(line, sizeof line, "connected [%s]:%u\n", host,
                          (unsigned)ntohs(v6->sin6_port));
    }
    if (length > 0 && (size_t)length < sizeof line) {
        po_log(line, (size_t)length);
    }
}

/* Whether an open carries a mode argument: only one that may create a file does. */
static int po_creates(int flags) {
#ifdef O_TMPFILE
    if ((flags & O_TMPFILE) == O_TMPFILE) {
        return 1;
    }
#endif
    return (flags & O_CREAT) != 0;
}

WRAPPER int WRAP(connect)(int fd, const struct sockaddr *address, socklen_t length) {
    po_connected(address);
    return REAL(connect)(fd, address, length);
}

WRAPPER int WRAP(open)(const char *path, int flags, ...) {
    mode_t mode = 0;
    if (po_creates(flags)) {
        va_list arguments;
        va_start(arguments, flags);
        mode = (mode_t)va_arg(arguments, int);
        va_end(arguments);
    }
    if (po_refused("open", AT_FDCWD, path)) {
        return -1;
    }
    return REAL(open)(path, flags, mode);
}

WRAPPER int WRAP(openat)(int dirfd, const char *path, int flags, ...) {
    mode_t mode = 0;
    if (po_creates(flags)) {
        va_list arguments;
        va_start(arguments, flags);
        mode = (mode_t)va_arg(arguments, int);
        va_end(arguments);
    }
    if (po_refused("openat", dirfd, path)) {
        return -1;
    }
    return REAL(openat)(dirfd, path, flags, mode);
}

WRAPPER int WRAP(stat)(const char *path, struct stat *buffer) {
    if (po_refused("stat", AT_FDCWD, path)) {
        return -1;
    }
    return REAL(stat)(path, buffer);
}

WRAPPER int WRAP(lstat)(const char *path, struct stat *buffer) {
    if (po_refused("lstat", AT_FDCWD, path)) {
        return -1;
    }
    return REAL(lstat)(path, buffer);
}

WRAPPER int WRAP(fstatat)(int dirfd, const char *path, struct stat *buffer, int flags) {
    if (po_refused("fstatat", dirfd, path)) {
        return -1;
    }
    return REAL(fstatat)(dirfd, path, buffer, flags);
}

WRAPPER int WRAP(access)(const char *path, int mode) {
    if (po_refused("access", AT_FDCWD, path)) {
        return -1;
    }
    return REAL(access)(path, mode);
}

WRAPPER int WRAP(faccessat)(int dirfd, const char *path, int mode, int flags) {
    if (po_refused("faccessat", dirfd, path)) {
        return -1;
    }
    return REAL(faccessat)(dirfd, path, mode, flags);
}

WRAPPER DIR *WRAP(opendir)(const char *path) {
    if (po_refused("opendir", AT_FDCWD, path)) {
        return NULL;
    }
    return REAL(opendir)(path);
}

#ifdef __APPLE__
INTERPOSE(connect)
INTERPOSE(open)
INTERPOSE(openat)
INTERPOSE(stat)
INTERPOSE(lstat)
INTERPOSE(fstatat)
INTERPOSE(access)
INTERPOSE(faccessat)
INTERPOSE(opendir)
#else
WRAPPER int WRAP(open64)(const char *path, int flags, ...) {
    mode_t mode = 0;
    if (po_creates(flags)) {
        va_list arguments;
        va_start(arguments, flags);
        mode = (mode_t)va_arg(arguments, int);
        va_end(arguments);
    }
    if (po_refused("open64", AT_FDCWD, path)) {
        return -1;
    }
    return REAL(open64)(path, flags, mode);
}

WRAPPER int WRAP(openat64)(int dirfd, const char *path, int flags, ...) {
    mode_t mode = 0;
    if (po_creates(flags)) {
        va_list arguments;
        va_start(arguments, flags);
        mode = (mode_t)va_arg(arguments, int);
        va_end(arguments);
    }
    if (po_refused("openat64", dirfd, path)) {
        return -1;
    }
    return REAL(openat64)(dirfd, path, flags, mode);
}

WRAPPER int WRAP(stat64)(const char *path, struct stat64 *buffer) {
    if (po_refused("stat64", AT_FDCWD, path)) {
        return -1;
    }
    return REAL(stat64)(path, buffer);
}

WRAPPER int WRAP(lstat64)(const char *path, struct stat64 *buffer) {
    if (po_refused("lstat64", AT_FDCWD, path)) {
        return -1;
    }
    return REAL(lstat64)(path, buffer);
}

WRAPPER int WRAP(fstatat64)(int dirfd, const char *path, struct stat64 *buffer, int flags) {
    if (po_refused("fstatat64", dirfd, path)) {
        return -1;
    }
    return REAL(fstatat64)(dirfd, path, buffer, flags);
}

WRAPPER int WRAP(statx)(int dirfd, const char *path, int flags, unsigned int mask,
                   struct statx *buffer) {
    if (po_refused("statx", dirfd, path)) {
        return -1;
    }
    return REAL(statx)(dirfd, path, flags, mask, buffer);
}
#endif
