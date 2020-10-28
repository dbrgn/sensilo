MEMORY
{
    /* NOTE K = KiBi = 1024 bytes */
    FLASH : ORIGIN = 0x00000000, LENGTH = 512K
    RAM : ORIGIN = 0x20000000, LENGTH = 63K

    /* Reserve 1 KiB of RAM for panic message dumps */
    PANDUMP: ORIGIN = 0x2000FC00, LENGTH = 1K
}

/* Used for panic-persist crate */
_panic_dump_start = ORIGIN(PANDUMP);
_panic_dump_end   = ORIGIN(PANDUMP) + LENGTH(PANDUMP);
