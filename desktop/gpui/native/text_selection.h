#ifndef THREADBEAR_TEXT_SELECTION_H
#define THREADBEAR_TEXT_SELECTION_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
// Rust's foreground queue owns request identities. The Swift translation
// anchor uses Clang-imported C declarations, with the same ABI as AppKit's
// Objective-C requestor, rather than declaring a Swift calling convention.
bool tb_services_return(uint64_t request, const unsigned char *words, size_t length);
void tb_services_discard(uint64_t request);
#endif
