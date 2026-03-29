// Minimal config.h for vendored liblzma build (Windows MSVC / cross-platform)
// This replaces autoconf-generated config.h with the minimal set of defines
// needed for a SquashFS decompression-focused build.

#ifndef LZMA_CONFIG_H
#define LZMA_CONFIG_H

// We have C99+ standard headers
#define HAVE_STDBOOL_H 1
#define HAVE_STDINT_H 1
#define HAVE_INTTYPES_H 1

// Enable decoders we need for SquashFS
#define HAVE_DECODER_LZMA1 1
#define HAVE_DECODER_LZMA2 1
#define HAVE_DECODER_DELTA 1
#define HAVE_DECODER_X86 1
#define HAVE_DECODER_ARM 1
#define HAVE_DECODER_ARM64 1
#define HAVE_DECODER_ARMTHUMB 1
#define HAVE_DECODER_SPARC 1
#define HAVE_DECODER_POWERPC 1
#define HAVE_DECODER_IA64 1
#define HAVE_DECODER_RISCV 1

// Enable encoders too (for writer support later)
#define HAVE_ENCODER_LZMA1 1
#define HAVE_ENCODER_LZMA2 1
#define HAVE_ENCODER_DELTA 1
#define HAVE_ENCODER_X86 1
#define HAVE_ENCODER_ARM 1
#define HAVE_ENCODER_ARM64 1
#define HAVE_ENCODER_ARMTHUMB 1
#define HAVE_ENCODER_SPARC 1
#define HAVE_ENCODER_POWERPC 1
#define HAVE_ENCODER_IA64 1
#define HAVE_ENCODER_RISCV 1

// Enable CRC checks (required for integrity verification)
#define HAVE_CHECK_CRC32 1
#define HAVE_CHECK_CRC64 1
#define HAVE_CHECK_SHA256 1

// Enable match finders for encoding
#define HAVE_MF_HC3 1
#define HAVE_MF_HC4 1
#define HAVE_MF_BT2 1
#define HAVE_MF_BT3 1
#define HAVE_MF_BT4 1

// No threading — SquashFS decompresses one block at a time
#define MYTHREAD_ENABLED 0

// Assume little-endian (Windows x86_64, ARM64)
// liblzma auto-detects this but we can be explicit
#ifndef WORDS_BIGENDIAN
// not defined = little endian
#endif

// sizeof(size_t) — needed by sysdefs.h fallback
#ifdef _WIN64
#define SIZEOF_SIZE_T 8
#else
#define SIZEOF_SIZE_T 4
#endif

#endif // LZMA_CONFIG_H
