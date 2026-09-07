; Use a final, explicit compressor configuration for deterministic NSIS output.
SetCompressor /FINAL /SOLID zlib
; Do not preserve source file last-write times in the NSIS archive.
; This keeps independently generated installers byte-for-byte reproducible.
SetDateSave off
