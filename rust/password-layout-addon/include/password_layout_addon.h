#pragma once
#include <stdbool.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct PasswordLayoutWorker PasswordLayoutWorker;  /* opaque */
PasswordLayoutWorker *password_layout_addon_start(void);   /* NULL on failure */
void password_layout_addon_want(PasswordLayoutWorker *worker, bool latin);
void password_layout_addon_stop(PasswordLayoutWorker *worker);
#ifdef __cplusplus
}
#endif
