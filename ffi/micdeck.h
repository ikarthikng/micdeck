// C view of the functions in src/lib.rs. Swift imports this header.
#include <stdbool.h>
#include <stdint.h>

// Lights. `effect` is solid, breathing, cycle, confetti, sun or twilight. Returns false for an unknown name.
bool md_light_start(const char *effect, uint8_t r, uint8_t g, uint8_t b, uint8_t brightness);
void md_light_stop(void);
bool md_light_running(void);

// Meeting cleanup: names are matched as "contains", strength is 0-100.
void md_meeting_start(const char *input, const char *output, uint8_t strength);
void md_meeting_stop(void);
bool md_meeting_running(void);

// Called on a background thread each time the mic's mute button is pressed. Call md_mute_watch once.
typedef void (*md_mute_callback)(void *ctx, bool muted);
void md_mute_watch(md_mute_callback callback, void *ctx);

// Returned strings must be freed with md_free_string. md_take_error returns NULL when there is no error.
char *md_take_error(void);
char *md_devices(bool inputs); // one device name per line
void md_free_string(char *s);
