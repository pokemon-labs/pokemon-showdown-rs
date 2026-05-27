/**
 * pokemon_showdown.h — C API for the Pokemon Showdown Rust Simulator
 *
 * Include this header in any C/C++ project that links against the compiled
 * `libpokemon_showdown` shared or static library.
 *
 * Typical compilation:
 *   gcc -o my_app my_app.c -L. -lpokemon_showdown -Wl,-rpath,'$ORIGIN'
 *
 * -------------------------------------------------------------------------
 * Quick start
 * -------------------------------------------------------------------------
 *
 *   PsBattleStream *stream = ps_stream_new();
 *
 *   ps_stream_receive(stream,
 *       ">start {\"formatid\":\"gen9randombattle\",\"seed\":\"gen5,deadbeef00112233\"}");
 *   ps_stream_receive(stream,
 *       ">player p1 {\"name\":\"Alice\",\"team\":\"\"}");
 *   ps_stream_receive(stream,
 *       ">player p2 {\"name\":\"Bob\",\"team\":\"\"}");
 *
 *   // drain initial output (requests, team preview, etc.)
 *   char *msg;
 *   while ((msg = ps_stream_read(stream)) != NULL) {
 *       printf("%s\n", msg);
 *       ps_string_free(msg);
 *   }
 *
 *   // send choices, poll output, repeat until battle ends
 *   while (!ps_stream_is_ended(stream)) {
 *       ps_stream_choose(stream, 1, "move 1");
 *       ps_stream_choose(stream, 2, "move 1");
 *       while ((msg = ps_stream_read(stream)) != NULL) {
 *           printf("%s\n", msg);
 *           ps_string_free(msg);
 *       }
 *   }
 *
 *   char *winner = ps_stream_winner(stream);
 *   if (winner) {
 *       printf("Winner: %s\n", winner);
 *       ps_string_free(winner);
 *   }
 *
 *   ps_stream_free(stream);
 */

#ifndef POKEMON_SHOWDOWN_H
#define POKEMON_SHOWDOWN_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* -------------------------------------------------------------------------
 * Error codes
 * ---------------------------------------------------------------------- */

/** Returned by functions that succeed. */
#define PS_OK            0
/** A required pointer argument was NULL. */
#define PS_ERR_NULL_PTR  (-1)
/** A string argument contained invalid UTF-8 or an embedded null byte. */
#define PS_ERR_INVALID_STRING (-2)
/** An unexpected internal error (e.g. Rust panic) occurred. */
#define PS_ERR_INTERNAL  (-3)

/* -------------------------------------------------------------------------
 * Opaque types
 * ---------------------------------------------------------------------- */

/**
 * Opaque handle to a BattleStream.
 *
 * Always obtain via ps_stream_new() or ps_stream_new_with_options(), and
 * release with ps_stream_free(). Do not allocate or inspect directly.
 */
typedef struct PsBattleStream PsBattleStream;

/* -------------------------------------------------------------------------
 * Error diagnostics
 * ---------------------------------------------------------------------- */

/**
 * ps_get_last_error — retrieve the last error from the calling thread.
 *
 * Returns a pointer to a null-terminated UTF-8 string describing the most
 * recent error on this thread, or NULL if no error has occurred.
 *
 * The pointer is valid until the next ps_* call on the same thread.
 * Do NOT free this pointer.
 *
 * @return  Error string, or NULL if no error.
 */
const char *ps_get_last_error(void);

/* -------------------------------------------------------------------------
 * BattleStream lifecycle
 * ---------------------------------------------------------------------- */

/**
 * ps_stream_new — create a BattleStream with default options.
 *
 * The caller owns the returned pointer and must release it with
 * ps_stream_free() when done.
 *
 * @return  Pointer to a new stream, or NULL on allocation failure.
 */
PsBattleStream *ps_stream_new(void);

/**
 * ps_stream_new_with_options — create a BattleStream with explicit options.
 *
 * @param debug        Non-zero to enable verbose debug output.
 * @param no_catch     Non-zero to let Rust panics propagate instead of being
 *                     caught and translated into error codes.
 * @param replay_mode  0 = off, 1 = spectator, 2 = full.
 * @param keep_alive   Non-zero to keep the stream alive after the battle ends.
 *
 * @return  Pointer to a new stream, or NULL on allocation failure.
 */
PsBattleStream *ps_stream_new_with_options(
    int debug,
    int no_catch,
    int replay_mode,
    int keep_alive
);

/**
 * ps_stream_free — destroy a BattleStream.
 *
 * Passing NULL is a safe no-op. Do not use the pointer after this call.
 *
 * @param stream  Stream to free.
 */
void ps_stream_free(PsBattleStream *stream);

/* -------------------------------------------------------------------------
 * BattleStream I/O
 * ---------------------------------------------------------------------- */

/**
 * ps_stream_receive — push protocol data into the stream.
 *
 * @p chunk may contain one or more newline-separated protocol lines, e.g.:
 *
 *   ps_stream_receive(s, ">start {\"formatid\":\"gen9randombattle\"}");
 *   ps_stream_receive(s, ">player p1 {\"name\":\"Alice\",\"team\":\"\"}");
 *   ps_stream_receive(s, ">p1 move 1\n>p2 move 2");
 *
 * @param stream  Valid non-null stream pointer.
 * @param chunk   Null-terminated UTF-8 protocol string.
 *
 * @return  PS_OK on success, negative error code on failure.
 */
int ps_stream_receive(PsBattleStream *stream, const char *chunk);

/**
 * ps_stream_read — pop one output message from the stream.
 *
 * Drains one pending output message.  Call in a loop until NULL is returned
 * to process all pending output before making further decisions.
 *
 * The returned string is heap-allocated and **must** be freed by the caller
 * with ps_string_free().
 *
 * @param stream  Valid non-null stream pointer.
 *
 * @return  Heap-allocated message string, or NULL if no messages are pending.
 *          Check ps_get_last_error() to distinguish errors from empty queues.
 */
char *ps_stream_read(PsBattleStream *stream);

/**
 * ps_stream_choose — send a player choice to the stream.
 *
 * Convenience wrapper that formats and sends ">p<slot> <choice>".
 *
 * @param stream       Valid non-null stream pointer.
 * @param player_slot  1 for player 1, 2 for player 2 (etc. for multi).
 * @param choice       Choice string, e.g. "move 1", "switch 3", "default".
 *
 * @return  PS_OK on success, negative error code on failure.
 */
int ps_stream_choose(PsBattleStream *stream, int player_slot, const char *choice);

/**
 * ps_stream_is_ended — query whether the battle has ended.
 *
 * @param stream  Valid non-null stream pointer.
 *
 * @return  Non-zero if the battle has ended, 0 otherwise.
 *          Returns 0 (and sets the error) if stream is NULL.
 */
int ps_stream_is_ended(const PsBattleStream *stream);

/**
 * ps_stream_winner — get the winner's name.
 *
 * Returns NULL if the battle has not ended or ended in a tie.
 *
 * The returned string is heap-allocated and **must** be freed with
 * ps_string_free().
 *
 * @param stream  Valid non-null stream pointer.
 *
 * @return  Heap-allocated winner name, or NULL.
 */
char *ps_stream_winner(const PsBattleStream *stream);

/* -------------------------------------------------------------------------
 * String memory management
 * ---------------------------------------------------------------------- */

/**
 * ps_string_free — free a string returned by this library.
 *
 * Passing NULL is a safe no-op. Do NOT call this on strings you allocated
 * yourself, and do NOT call it on the pointer returned by ps_get_last_error().
 *
 * @param s  String previously returned by ps_stream_read(), ps_stream_winner(),
 *           or any other ps_* function that allocates a string.
 */
void ps_string_free(char *s);

/* -------------------------------------------------------------------------
 * Diagnostics
 * ---------------------------------------------------------------------- */

/**
 * ps_version — return the simulator version string.
 *
 * @return  Static null-terminated string, e.g. "pokemon-showdown-rs 0.1.0".
 *          Valid for the lifetime of the process; do NOT free.
 */
const char *ps_version(void);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* POKEMON_SHOWDOWN_H */
