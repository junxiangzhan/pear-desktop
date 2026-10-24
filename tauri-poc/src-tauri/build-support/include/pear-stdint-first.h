/* Force-included into Android C builds by .cargo/config.toml: mp3lame-sys
   builds LAME with its MSVC config header whenever the host is Windows, and
   under clang that header #defines uint8_t and friends, which breaks
   <stdint.h> when it is parsed afterwards. Parsing it first makes the macros
   harmless. Assembly files (ring) must not see C headers. */
#ifndef __ASSEMBLER__
#include <stdint.h>
#endif
