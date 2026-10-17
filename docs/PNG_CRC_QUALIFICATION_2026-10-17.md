# PNG CRC verification qualification

Implementation: `d5857f569ec010f7936e8b26caa3e57542927ba6`.

- Rust workspace CI: run 39025195816, passed.
- Platform shells CI: run 39025195930, passed.
- Earlier source SHA `aed3a19597384c4f1c4b4c5f3219d20cc4021a5c` failed Rust formatting in run 39022159446, job 117365955407, step 4.
- Ralph Bridge source repair operation `e16d2811ba8d4e9e861ea8658e6c9d5d` accepted and verified.
- The full canonical TODO remains 1,536 lines, original blob `ec594ed49cf4fff5e1c7ab42fca927881be55df5`. SRC-003 census blob `0191d49981243930fce5039d03225248068ac51d` is preserved.

This opt-in PNG chunk CRC guard is not an image decoder, media sanitizer, authenticity certificate or offline rendering acceptance. PACK-003 remains unchecked.
