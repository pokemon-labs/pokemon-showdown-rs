/**
 * example_battle.c — Demonstrates the Pokemon Showdown C FFI
 *
 * Build (after `cargo build --release`):
 *
 *   gcc -o example_battle example_battle.c \
 *       -I../include \
 *       -L../target/release \
 *       -lpokemon_showdown \
 *       -Wl,-rpath,'$ORIGIN/../target/release'
 *
 * Run:
 *   ./example_battle
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "pokemon_showdown.h"

/* Drain and print all pending messages from the stream. */
static void drain(PsBattleStream *stream) {
    char *msg;
    while ((msg = ps_stream_read(stream)) != NULL) {
        printf("  OUT: %s\n", msg);
        ps_string_free(msg);
    }
    /* Check whether read stopped because of an error */
    const char *err = ps_get_last_error();
    if (err) {
        fprintf(stderr, "  [read error] %s\n", err);
    }
}

int main(void) {
    printf("Pokemon Showdown Simulator — version: %s\n\n", ps_version());

    /* ------------------------------------------------------------------
     * 1. Create a stream
     * ------------------------------------------------------------------ */
    PsBattleStream *stream = ps_stream_new();
    if (!stream) {
        fprintf(stderr, "Failed to create stream: %s\n", ps_get_last_error());
        return 1;
    }

    /* ------------------------------------------------------------------
     * 2. Initialise a Gen 9 Random Battle with a fixed seed
     * ------------------------------------------------------------------ */
    int rc;

    rc = ps_stream_receive(stream,
        ">start {\"formatid\":\"gen9randombattle\","
        "\"seed\":\"gen5,deadbeef00112233\"}");
    if (rc != PS_OK) {
        fprintf(stderr, "start failed: %s\n", ps_get_last_error());
        ps_stream_free(stream);
        return 1;
    }

    rc = ps_stream_receive(stream,
        ">player p1 {\"name\":\"Alice\",\"team\":\"\"}");
    if (rc != PS_OK) {
        fprintf(stderr, "p1 setup failed: %s\n", ps_get_last_error());
        ps_stream_free(stream);
        return 1;
    }

    rc = ps_stream_receive(stream,
        ">player p2 {\"name\":\"Bob\",\"team\":\"\"}");
    if (rc != PS_OK) {
        fprintf(stderr, "p2 setup failed: %s\n", ps_get_last_error());
        ps_stream_free(stream);
        return 1;
    }

    printf("--- Initial output (team preview / requests) ---\n");
    drain(stream);
    printf("\n");

    /* ------------------------------------------------------------------
     * 3. Auto-play: always choose the first available action
     * ------------------------------------------------------------------ */
    int turn = 0;
    while (!ps_stream_is_ended(stream) && turn < 200) {
        turn++;
        printf("--- Turn %d ---\n", turn);

        /* Both players pick move 1 (or switch 1 if forced). */
        rc = ps_stream_choose(stream, 1, "move 1");
        if (rc != PS_OK) {
            /* If move 1 is illegal the engine will send an error request;
               fall back to 'default' (let engine auto-choose). */
            ps_stream_choose(stream, 1, "default");
        }

        rc = ps_stream_choose(stream, 2, "move 1");
        if (rc != PS_OK) {
            ps_stream_choose(stream, 2, "default");
        }

        drain(stream);
        printf("\n");
    }

    /* ------------------------------------------------------------------
     * 4. Print result
     * ------------------------------------------------------------------ */
    if (ps_stream_is_ended(stream)) {
        char *winner = ps_stream_winner(stream);
        if (winner) {
            printf("Battle over! Winner: %s\n", winner);
            ps_string_free(winner);
        } else {
            printf("Battle over! Result: tie (or no winner recorded)\n");
        }
    } else {
        printf("Reached turn limit without a winner.\n");
    }

    /* ------------------------------------------------------------------
     * 5. Clean up
     * ------------------------------------------------------------------ */
    ps_stream_free(stream);
    return 0;
}
