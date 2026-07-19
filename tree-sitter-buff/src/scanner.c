/**
 * @file scanner.c — external scanner for the Buff tree-sitter grammar.
 *
 * Implements the offside (indentation) rule: emits synthetic NEWLINE,
 * INDENT, DEDENT tokens based on leading-whitespace changes between
 * non-blank lines.
 *
 * The scanner is queried whenever one of NEWLINE/INDENT/DEDENT is a valid
 * continuation. In every other context the parser falls back to the
 * `extras: [/[\\s]/, $.comment]` rule, so newlines inside `()`/`[]`/`{}`
 * (where none of the three external tokens is valid) are silently
 * consumed as trivia. That is how bracket-depth suppression is achieved
 * without making `)`, `]`, `}` themselves external tokens.
 *
 * Indentation rules mirrored from the hand-rolled lexer at
 * `crates/buff-lang-lexer/src/indent.rs`:
 *  - Spaces count 1 column each.
 *  - Tabs are NOT recognised as indent — a tab terminates the indent run
 *    (Buff mandates 4-space indentation; tabs are a syntax error in the
 *    authoritative lexer).
 *
 * Token semantics (per tree-sitter convention):
 *  - All three tokens are zero-width; the bytes they "skip" (newline +
 *    indent whitespace) are marked as trivia via `advance(true)` and
 *    `mark_end` at the original position. This lets the parser freely
 *    query the scanner for any of the three tokens at any whitespace
 *    position without the byte accounting getting confused.
 *
 * State serialization/deserialization is implemented so the parser can
 * recover from re-parses (incremental editing in editors).
 */

#include "tree_sitter/parser.h"

#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

// --------------------------------------------------------------------
// Token types — order MUST match the `externals` array in grammar.js.
// --------------------------------------------------------------------
enum TokenType {
  TOKEN_NEWLINE = 0,
  TOKEN_INDENT,
  TOKEN_DEDENT,
};

// Maximum supported indentation depth (in indent levels, not columns).
#define MAX_INDENT_DEPTH 256

// --------------------------------------------------------------------
// Scanner state
// --------------------------------------------------------------------
typedef struct {
  // Indent stack in column units. `stack[0]` is always 0 (file scope).
  uint16_t stack[MAX_INDENT_DEPTH];
  uint32_t stack_len;
} Scanner;

// --------------------------------------------------------------------
// Small helpers
// --------------------------------------------------------------------

static inline void skip(TSLexer *lexer) { lexer->advance(lexer, true); }

static bool scanner_push(Scanner *s, uint16_t level) {
  if (s->stack_len >= MAX_INDENT_DEPTH) return false;
  s->stack[s->stack_len++] = level;
  return true;
}

static void scanner_pop(Scanner *s) {
  if (s->stack_len <= 1) return;
  s->stack_len--;
}

static uint16_t scanner_top(Scanner *s) {
  if (s->stack_len == 0) return 0;
  return s->stack[s->stack_len - 1];
}

// --------------------------------------------------------------------
// Tree-sitter external scanner API
// --------------------------------------------------------------------

void *tree_sitter_buff_external_scanner_create(void) {
  Scanner *s = (Scanner *)calloc(1, sizeof(Scanner));
  if (s == NULL) return NULL;
  s->stack[0] = 0;
  s->stack_len = 1;
  return (void *)s;
}

void tree_sitter_buff_external_scanner_destroy(void *payload) {
  free(payload);
}

unsigned tree_sitter_buff_external_scanner_serialize(void *payload, char *buffer) {
  Scanner *s = (Scanner *)payload;
  if (s == NULL || s->stack_len == 0) return 0;

  // We write stack[1..stack_len-1] as uint16_t LE pairs. stack[0] is
  // always 0 and is implicit.
  uint32_t entries = s->stack_len - 1;
  if (entries > 127) entries = 127;

  size_t needed = 1 + entries * 2;
  if (needed > TREE_SITTER_SERIALIZATION_BUFFER_SIZE) {
    entries = (TREE_SITTER_SERIALIZATION_BUFFER_SIZE - 1) / 2;
  }

  buffer[0] = (char)(uint8_t)entries;
  for (uint32_t i = 0; i < entries; i++) {
    uint16_t v = s->stack[i + 1];
    buffer[1 + i * 2]     = (char)(uint8_t)(v & 0xFF);
    buffer[1 + i * 2 + 1] = (char)(uint8_t)((v >> 8) & 0xFF);
  }
  return (unsigned)(1 + entries * 2);
}

void tree_sitter_buff_external_scanner_deserialize(void *payload, const char *buffer, unsigned length) {
  Scanner *s = (Scanner *)payload;
  if (s == NULL) return;

  s->stack[0] = 0;
  s->stack_len = 1;

  if (length == 0 || buffer == NULL) return;

  uint8_t entries = (uint8_t)buffer[0];
  size_t available = length - 1;
  if (available < (size_t)entries * 2) {
    entries = (uint8_t)(available / 2);
  }

  for (uint32_t i = 0; i < entries && s->stack_len < MAX_INDENT_DEPTH; i++) {
    uint16_t lo = (uint8_t)buffer[1 + i * 2];
    uint16_t hi = (uint8_t)buffer[1 + i * 2 + 1];
    s->stack[s->stack_len++] = (uint16_t)(lo | (hi << 8));
  }
}

// --------------------------------------------------------------------
// The main scan function
// --------------------------------------------------------------------
bool tree_sitter_buff_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid_symbols) {
  Scanner *s = (Scanner *)payload;
  if (s == NULL) return false;

  // If none of our tokens are valid, defer to the internal scanner
  // (which will consume whitespace via `extras`).
  if (!valid_symbols[TOKEN_NEWLINE] && !valid_symbols[TOKEN_INDENT] && !valid_symbols[TOKEN_DEDENT]) {
    return false;
  }

  // Mark the END of the token at the current position BEFORE we skip
  // any whitespace. This makes the emitted token zero-width: the bytes
  // scanned below are treated as `extras` (trivia) by the parser.
  //
  // Rationale: NEWLINE/INDENT/DEDENT are virtual tokens; they do not
  // own bytes. The actual whitespace they recognise is trivia.
  lexer->mark_end(lexer);

  // -----------------------------------------------------------------
  // Scan forward through whitespace + newlines + blank lines to
  // determine:
  //   - did we cross at least one newline?  (`crossed_newline`)
  //   - what is the indent of the most recent line we scanned?
  //     (`indent_length`)
  //   - did we hit EOF?  (`at_eof`)
  //
  // Tabs terminate the indent run (Buff rejects tabs in indent — the
  // authoritative lexer errors on them; here we surface that as a
  // parse error by mis-counting indent).
  // -----------------------------------------------------------------
  bool crossed_newline = false;
  bool at_eof = false;
  uint16_t indent_length = 0;

  for (;;) {
    int32_t c = lexer->lookahead;

    if (c == 0) {
      // EOF.
      at_eof = true;
      break;
    }

    if (c == '\n') {
      crossed_newline = true;
      indent_length = 0;
      skip(lexer);
      continue;
    }

    if (c == '\r') {
      // \r or \r\n — treat as a newline.
      crossed_newline = true;
      indent_length = 0;
      skip(lexer);
      // Consume \n if it follows (\r\n pair).
      if (lexer->lookahead == '\n') {
        skip(lexer);
      }
      continue;
    }

    if (c == ' ') {
      indent_length++;
      skip(lexer);
      continue;
    }

    if (c == '\f') {
      // Form feed — reset indent (some editors use it for tidy).
      indent_length = 0;
      skip(lexer);
      continue;
    }

    if (c == '\t') {
      // Tab in leading position is invalid in Buff. Stop scanning so
      // the indent_length reflects only the spaces seen so far. The
      // parser will fail to match, surfacing the error.
      break;
    }

    // Any other byte (content, comment, etc.) — stop scanning.
    break;
  }

  uint16_t current = scanner_top(s);

  // -----------------------------------------------------------------
  // Decide which token to emit.
  // Order: INDENT > DEDENT > NEWLINE > trailing-DEDENT-at-EOF.
  // -----------------------------------------------------------------

  // INDENT: next line is more indented than the current block.
  if (valid_symbols[TOKEN_INDENT] && crossed_newline && !at_eof && indent_length > current) {
    scanner_push(s, indent_length);
    lexer->result_symbol = TOKEN_INDENT;
    return true;
  }

  // DEDENT: next line is less indented than the current block.
  if (valid_symbols[TOKEN_DEDENT] && (crossed_newline || at_eof) && indent_length < current) {
    scanner_pop(s);
    lexer->result_symbol = TOKEN_DEDENT;
    return true;
  }

  // NEWLINE: indent unchanged.
  if (valid_symbols[TOKEN_NEWLINE] && crossed_newline && indent_length == current) {
    lexer->result_symbol = TOKEN_NEWLINE;
    return true;
  }

  // At EOF, drain any remaining indent levels as DEDENTs.
  if (valid_symbols[TOKEN_DEDENT] && at_eof && current > 0) {
    scanner_pop(s);
    lexer->result_symbol = TOKEN_DEDENT;
    return true;
  }

  // Final NEWLINE at EOF (so the parser can close any pending stmt).
  if (valid_symbols[TOKEN_NEWLINE] && at_eof) {
    lexer->result_symbol = TOKEN_NEWLINE;
    return true;
  }

  return false;
}
