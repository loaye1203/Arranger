/* Native bundle entry: replace this process with the validated GUI binary. */
#include <mach-o/dyld.h>
#include <limits.h>
#include <pwd.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv) {
    char executable[PATH_MAX], resolved[PATH_MAX], binary[PATH_MAX];
    char temporary[PATH_MAX], endpoint[PATH_MAX];
    uint32_t size = sizeof(executable);
    if (_NSGetExecutablePath(executable, &size) != 0 || !realpath(executable, resolved)) {
        fputs("Arranger: cannot locate application bundle\n", stderr);
        return 2;
    }
    char *slash = strrchr(resolved, '/');
    if (!slash) return 2;
    *slash = '\0';
    if (snprintf(binary, sizeof(binary), "%s/arranger-bin", resolved) >= (int)sizeof(binary)) return 2;
    size_t count = confstr(_CS_DARWIN_USER_TEMP_DIR, temporary, sizeof(temporary));
    if (!count || count > sizeof(temporary) ||
        snprintf(endpoint, sizeof(endpoint), "%sorg.arranger.desktop-%u-%ld.json",
                 temporary, (unsigned)getuid(), (long)getpid()) >= (int)sizeof(endpoint)) {
        fputs("Arranger: cannot locate user temporary directory\n", stderr);
        return 2;
    }
    /* Keep the process in the user's home, outside the bundle or image.
       Entering Documents here can make the first getcwd() block before the
       native file sheet opens. Let that sheet handle user-chosen folders. */
    const char *home = getenv("HOME");
    if (!home || home[0] != '/') {
        struct passwd *user = getpwuid(getuid());
        home = user ? user->pw_dir : NULL;
    }
    if (!home || home[0] != '/') { fputs("Arranger: cannot locate user directory\n", stderr); return 2; }
    if (chdir(home) != 0) { perror("Arranger: user directory"); return 2; }
    char **arguments = calloc((size_t)argc + 4, sizeof(*arguments));
    if (!arguments) return 2;
    arguments[0] = binary; arguments[1] = "gui";
    arguments[2] = "--endpoint"; arguments[3] = endpoint;
    for (int i = 1; i < argc; ++i) arguments[i + 3] = argv[i];
    execv(binary, arguments);
    perror("Arranger: launch GUI");
    free(arguments);
    return 2;
}
